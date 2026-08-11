use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::Notify;

use concurrent_queue::{ConcurrentQueue, PopError, PushError};

use crate::limiter::buffer_limiter::BufferLimiter;

use super::dt_data::DtItem;

pub struct DtQueue {
    queue: ConcurrentQueue<DtItem>,
    check_memory: bool,
    max_bytes: u64,
    cur_bytes: AtomicU64,
    not_full: Arc<Notify>,
    enqueue_limiter: Option<Arc<BufferLimiter>>,
    dequeue_limiter: Option<Arc<BufferLimiter>>,
}

impl DtQueue {
    pub fn new(
        capacity: usize,
        max_bytes: u64,
        enqueue_limiter: Option<Arc<BufferLimiter>>,
        dequeue_limiter: Option<Arc<BufferLimiter>>,
    ) -> Self {
        Self {
            queue: ConcurrentQueue::bounded(capacity),
            max_bytes,
            check_memory: max_bytes > 0,
            cur_bytes: AtomicU64::new(0),
            not_full: Arc::new(Notify::new()),
            enqueue_limiter,
            dequeue_limiter,
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.queue.is_full()
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    #[inline(always)]
    pub fn get_curr_size(&self) -> u64 {
        self.cur_bytes.load(Ordering::Acquire)
    }

    pub async fn push(&self, mut item: DtItem) -> anyhow::Result<()> {
        if let Some(enqueue_limiter) = &self.enqueue_limiter {
            enqueue_limiter.acquire(&item).await?;
        }
        let item_size = item.dt_data.get_data_size();
        loop {
            if !self.queue.is_full() && !self.is_mem_full() {
                self.cur_bytes.fetch_add(item_size, Ordering::AcqRel);
                let res = self.queue.push(item);
                match res {
                    Ok(_) => return Ok(()),
                    Err(PushError::Full(returned_item)) => {
                        self.subtract_bytes(item_size);
                        item = returned_item;
                    }
                    Err(e) => {
                        self.subtract_bytes(item_size);
                        return Err(e.into());
                    }
                }
            }
            self.not_full.notified().await;
        }
    }

    pub async fn pop(&self) -> anyhow::Result<DtItem, PopError> {
        let item = self.queue.pop()?;

        if let Some(enqueue_limiter) = &self.enqueue_limiter {
            enqueue_limiter.release(&item).await;
        }
        if let Some(dequeue_limiter) = &self.dequeue_limiter {
            // error can not be returned here, the item has been popped out,
            // and the limiter acquire should not fail.
            dequeue_limiter.acquire(&item).await.unwrap();
            dequeue_limiter.release(&item).await;
        }

        self.subtract_bytes(item.dt_data.get_data_size());

        self.not_full.notify_one();

        Ok(item)
    }

    fn subtract_bytes(&self, bytes: u64) {
        let mut current = self.cur_bytes.load(Ordering::Acquire);
        loop {
            let next = current.saturating_sub(bytes);
            match self.cur_bytes.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    #[inline(always)]
    fn is_mem_full(&self) -> bool {
        if self.check_memory {
            self.cur_bytes.load(Ordering::Acquire) > self.max_bytes
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Barrier as ThreadBarrier,
        },
        time::Duration,
    };

    use concurrent_queue::PopError;
    use tokio::sync::Barrier as AsyncBarrier;

    use crate::meta::{
        dt_data::{DtData, DtItem},
        foxlake::s3_file_meta::S3FileMeta,
        position::Position,
    };

    use super::DtQueue;

    fn bytes_item(data_size: usize) -> DtItem {
        DtItem {
            dt_data: DtData::Foxlake {
                file_meta: S3FileMeta {
                    data_size,
                    ..Default::default()
                },
            },
            position: Position::None,
            data_origin_node: "test".to_string(),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn visible_items_are_accounted_during_concurrent_pushes() {
        const PRODUCERS: usize = 4;
        const ITEMS_PER_PRODUCER: usize = 100_000;
        const ITEM_BYTES: u64 = 1;
        const TOTAL_ITEMS: usize = PRODUCERS * ITEMS_PER_PRODUCER;

        let queue = Arc::new(DtQueue::new(TOTAL_ITEMS, 0, None, None));
        let start = Arc::new(ThreadBarrier::new(2));
        let stop_observer = Arc::new(AtomicBool::new(false));
        let saw_unaccounted_item = Arc::new(AtomicBool::new(false));

        let observer_queue = queue.clone();
        let observer_start = start.clone();
        let observer_stop = stop_observer.clone();
        let observer_saw_unaccounted_item = saw_unaccounted_item.clone();
        let observer = std::thread::spawn(move || {
            observer_start.wait();
            while !observer_stop.load(Ordering::Acquire) {
                let visible_items = observer_queue.len() as u64;
                let accounted_bytes = observer_queue.get_curr_size();
                if accounted_bytes < visible_items * ITEM_BYTES {
                    observer_saw_unaccounted_item.store(true, Ordering::Release);
                    break;
                }
                std::hint::spin_loop();
            }
        });

        start.wait();
        let producers: Vec<_> = (0..PRODUCERS)
            .map(|_| {
                let queue = queue.clone();
                tokio::spawn(async move {
                    for _ in 0..ITEMS_PER_PRODUCER {
                        queue.push(bytes_item(ITEM_BYTES as usize)).await.unwrap();
                    }
                })
            })
            .collect();

        for producer in producers {
            producer.await.unwrap();
        }
        stop_observer.store(true, Ordering::Release);
        observer.join().unwrap();

        assert!(
            !saw_unaccounted_item.load(Ordering::Acquire),
            "an item became visible before its bytes were accounted"
        );
        assert_eq!(queue.len(), TOTAL_ITEMS);
        assert_eq!(queue.get_curr_size(), TOTAL_ITEMS as u64 * ITEM_BYTES);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn byte_accounting_returns_to_zero_after_concurrent_push_and_pop() {
        const PRODUCERS: usize = 4;
        const CONSUMERS: usize = 4;
        const ITEMS_PER_PRODUCER: usize = 25_000;
        const TOTAL_ITEMS: usize = PRODUCERS * ITEMS_PER_PRODUCER;
        const QUEUE_CAPACITY: usize = 64;
        const MAX_ACCOUNTED_BYTES: u64 =
            QUEUE_CAPACITY as u64 + PRODUCERS as u64 + CONSUMERS as u64;

        let queue = Arc::new(DtQueue::new(
            QUEUE_CAPACITY,
            QUEUE_CAPACITY as u64,
            None,
            None,
        ));
        let start = Arc::new(AsyncBarrier::new(PRODUCERS + CONSUMERS + 1));
        let consumed = Arc::new(AtomicUsize::new(0));
        let stop_observer = Arc::new(AtomicBool::new(false));
        let saw_invalid_count = Arc::new(AtomicBool::new(false));

        let observer_queue = queue.clone();
        let observer_start = start.clone();
        let observer_stop = stop_observer.clone();
        let observer_saw_invalid_count = saw_invalid_count.clone();
        let observer = tokio::spawn(async move {
            observer_start.wait().await;
            while !observer_stop.load(Ordering::Acquire) {
                if observer_queue.get_curr_size() > MAX_ACCOUNTED_BYTES {
                    observer_saw_invalid_count.store(true, Ordering::Release);
                    break;
                }
                tokio::task::yield_now().await;
            }
        });

        let consumers: Vec<_> = (0..CONSUMERS)
            .map(|_| {
                let queue = queue.clone();
                let start = start.clone();
                let consumed = consumed.clone();
                tokio::spawn(async move {
                    start.wait().await;
                    while consumed.load(Ordering::Acquire) < TOTAL_ITEMS {
                        match queue.pop().await {
                            Ok(_) => {
                                consumed.fetch_add(1, Ordering::AcqRel);
                            }
                            Err(PopError::Empty) => tokio::task::yield_now().await,
                            Err(PopError::Closed) => {
                                panic!("queue closed before all items were consumed")
                            }
                        }
                    }
                })
            })
            .collect();

        let producers: Vec<_> = (0..PRODUCERS)
            .map(|_| {
                let queue = queue.clone();
                let start = start.clone();
                tokio::spawn(async move {
                    start.wait().await;
                    for _ in 0..ITEMS_PER_PRODUCER {
                        queue.push(bytes_item(1)).await.unwrap();
                    }
                })
            })
            .collect();

        let run_result = tokio::time::timeout(Duration::from_secs(10), async {
            for producer in producers {
                producer.await.unwrap();
            }
            for consumer in consumers {
                consumer.await.unwrap();
            }
        })
        .await;

        stop_observer.store(true, Ordering::Release);
        observer.await.unwrap();
        run_result.expect("concurrent push/pop stalled");

        assert!(
            !saw_invalid_count.load(Ordering::Acquire),
            "byte accounting exceeded the queue plus in-flight reservations"
        );
        assert!(queue.is_empty());
        assert_eq!(queue.get_curr_size(), 0);
    }
}
