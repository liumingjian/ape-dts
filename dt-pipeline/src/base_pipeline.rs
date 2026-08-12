use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::{
    sync::{Mutex, RwLock},
    task::yield_now,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use crate::{lua_processor::LuaProcessor, Pipeline};
use dt_common::{
    config::sinker_config::SinkerConfig,
    log_error, log_info, log_position,
    meta::{
        dcl_meta::dcl_data::DclData,
        ddl_meta::ddl_data::DdlData,
        dt_data::{DtData, DtItem},
        dt_queue::DtQueue,
        position::Position,
        row_data::RowData,
        syncer::Syncer,
    },
    monitor::{counter_type::CounterType, monitor::Monitor},
};
use dt_connector::{data_marker::DataMarker, extractor::resumer::recorder::Recorder, Sinker};
use dt_parallelizer::{DataSize, Parallelizer};

pub struct BasePipeline {
    pub buffer: Arc<DtQueue>,
    pub parallelizer: Box<dyn Parallelizer + Send + Sync>,
    pub sinker_config: SinkerConfig,
    pub sinkers: Vec<Arc<async_mutex::Mutex<Box<dyn Sinker + Send>>>>,
    /// Cancelled when the task is shutting down, whichever side triggered it.
    pub cancel_token: CancellationToken,
    pub checkpoint_interval_secs: u64,
    pub batch_sink_interval_secs: u64,
    pub syncer: Arc<Mutex<Syncer>>,
    pub monitor: Arc<Monitor>,
    pub data_marker: Option<Arc<RwLock<DataMarker>>>,
    pub lua_processor: Option<LuaProcessor>,
    pub recorder: Option<Arc<dyn Recorder + Send + Sync>>,
}

/// How long an idle pipeline parks before re-checking its timers. New data wakes it
/// immediately through the queue, so this only bounds the checkpoint / batch-sink clocks.
const IDLE_WAIT: Duration = Duration::from_millis(100);

enum SinkMethod {
    Raw,
    Ddl,
    Dcl,
    Dml,
    Struct,
}

#[async_trait]
impl Pipeline for BasePipeline {
    async fn stop(&mut self) -> anyhow::Result<()> {
        for sinker in self.sinkers.iter_mut() {
            sinker.lock().await.close().await?;
        }
        self.parallelizer.close().await
    }

    async fn start(&mut self) -> anyhow::Result<()> {
        log_info!(
            "{} starts, parallel_size: {}, checkpoint_interval_secs: {}",
            self.parallelizer.get_name(),
            self.sinkers.len(),
            self.checkpoint_interval_secs
        );

        let mut last_sink_time = Instant::now();
        let mut last_checkpoint_time = Instant::now();
        let mut last_received_position = Position::None;
        let mut last_commit_position = Position::None;
        let mut record_time = Instant::now();

        // the parallelizer can hold items it already popped off the queue, so an empty
        // queue alone does not mean everything has been sinked
        while !self.cancel_token.is_cancelled()
            || !self.buffer.is_empty()
            || self.parallelizer.has_pending_data()
        {
            // to avoid too many sub counters, only add counter when buffer is not empty
            if !self.buffer.is_empty() {
                self.monitor
                    .add_counter(CounterType::BufferSize, self.buffer.len() as u64)
                    .await;
            }
            if record_time.elapsed().as_secs() > 1 {
                let len = self.buffer.len() as u64;
                let size = self.buffer.get_curr_size();
                self.monitor
                    .set_counter(CounterType::QueuedRecordCurrent, len);
                self.monitor
                    .set_counter(CounterType::QueuedByteCurrent, size);
                record_time = Instant::now();
            }

            // some sinkers (foxlake) need to accumulate data to a big batch and sink
            let accumulating = last_sink_time.elapsed().as_secs() < self.batch_sink_interval_secs
                && !self.buffer.is_full();
            let data = if accumulating {
                Vec::new()
            } else {
                last_sink_time = Instant::now();
                self.parallelizer.drain(self.buffer.as_ref()).await?
            };
            let drained_count = data.len();

            if let Some(data_marker) = &mut self.data_marker {
                if !data.is_empty() {
                    data_marker.write().await.data_origin_node = data[0].data_origin_node.clone();
                }
            }

            // process all row_data_items in buffer at a time
            let (data_size, last_received, last_commit) = match self.get_sink_method(&data) {
                SinkMethod::Ddl => self.sink_ddl(data).await?,
                SinkMethod::Dcl => self.sink_dcl(data).await?,
                SinkMethod::Dml => self.sink_dml(data).await?,
                SinkMethod::Raw => self.sink_raw(data).await?,
                SinkMethod::Struct => self.sink_struct(data).await?,
            };

            if let Some(position) = &last_received {
                self.syncer.lock().await.received_position = position.to_owned();
                last_received_position = position.to_owned();
            }
            if let Some(position) = &last_commit {
                last_commit_position = position.to_owned();
            }

            last_checkpoint_time = self
                .record_checkpoint(
                    Some(last_checkpoint_time),
                    &last_received_position,
                    &last_commit_position,
                )
                .await;

            self.monitor
                .add_counter(CounterType::SinkedRecordTotal, data_size.count)
                .await
                .add_counter(CounterType::SinkedByteTotal, data_size.bytes)
                .await;

            if self.cancel_token.is_cancelled() || drained_count > 0 {
                // still making progress (or draining out on shutdown): keep the loop hot
                yield_now().await;
            } else if accumulating {
                // holding data back until batch_sink_interval_secs elapses: only the clock
                // can end this round, so sleep out the rest of the accumulation window
                self.sleep_while_accumulating(last_sink_time).await;
            } else {
                // the queue was empty: park on it instead of spinning, otherwise an idle
                // cdc task burns a whole core on this loop
                self.buffer.wait_for_data(IDLE_WAIT).await;
            }
        }

        self.record_checkpoint(None, &last_received_position, &last_commit_position)
            .await;
        Ok(())
    }
}

impl BasePipeline {
    /// Sleep out the rest of the batch-sink accumulation window, never past [`IDLE_WAIT`]
    /// and never past shutdown.
    async fn sleep_while_accumulating(&self, last_sink_time: Instant) {
        let elapsed = last_sink_time.elapsed();
        let window = Duration::from_secs(self.batch_sink_interval_secs);
        let remaining = window.saturating_sub(elapsed).min(IDLE_WAIT);
        if remaining.is_zero() {
            return;
        }
        tokio::select! {
            _ = self.cancel_token.cancelled() => {}
            _ = tokio::time::sleep(remaining) => {}
        }
    }

    async fn sink_raw(
        &mut self,
        all_data: Vec<DtItem>,
    ) -> anyhow::Result<(DataSize, Option<Position>, Option<Position>)> {
        let (data_count, last_received_position, last_commit_position) = Self::fetch_raw(&all_data);
        if data_count > 0 {
            let data_size = self.parallelizer.sink_raw(all_data, &self.sinkers).await?;
            Ok((data_size, last_received_position, last_commit_position))
        } else {
            Ok((
                DataSize::default(),
                last_received_position,
                last_commit_position,
            ))
        }
    }

    async fn sink_struct(
        &mut self,
        mut all_data: Vec<DtItem>,
    ) -> anyhow::Result<(DataSize, Option<Position>, Option<Position>)> {
        let mut data = Vec::new();
        for i in all_data.drain(..) {
            if let DtData::Struct { struct_data } = i.dt_data {
                data.push(struct_data);
            }
        }
        let data_size = self.parallelizer.sink_struct(data, &self.sinkers).await?;
        Ok((data_size, None, None))
    }

    async fn sink_dml(
        &mut self,
        all_data: Vec<DtItem>,
    ) -> anyhow::Result<(DataSize, Option<Position>, Option<Position>)> {
        let (mut data, last_received_position, last_commit_position) = Self::fetch_dml(all_data);
        if !data.is_empty() {
            // execute lua processor
            if let Some(lua_processor) = &self.lua_processor {
                data = lua_processor.process(data)?;
            }

            let data_size = self.parallelizer.sink_dml(data, &self.sinkers).await?;
            Ok((data_size, last_received_position, last_commit_position))
        } else {
            Ok((
                DataSize::default(),
                last_received_position,
                last_commit_position,
            ))
        }
    }

    async fn sink_ddl(
        &mut self,
        all_data: Vec<DtItem>,
    ) -> anyhow::Result<(DataSize, Option<Position>, Option<Position>)> {
        let (data, last_received_position, last_commit_position) = Self::fetch_ddl(all_data);
        if !data.is_empty() {
            let data_size = self
                .parallelizer
                .sink_ddl(data.clone(), &self.sinkers)
                .await?;
            // only part of sinkers will execute sink_ddl, but all sinkers should refresh metadata
            for sinker in self.sinkers.iter_mut() {
                sinker.lock().await.refresh_meta(data.clone()).await?;
            }
            self.monitor
                .add_counter(CounterType::DDLRecordTotal, data_size.count)
                .await;
            Ok((data_size, last_received_position, last_commit_position))
        } else {
            Ok((
                DataSize::default(),
                last_received_position,
                last_commit_position,
            ))
        }
    }

    async fn sink_dcl(
        &mut self,
        all_data: Vec<DtItem>,
    ) -> anyhow::Result<(DataSize, Option<Position>, Option<Position>)> {
        let (data, last_received_position, last_commit_position) = Self::fetch_dcl(all_data);
        let data_size = DataSize {
            count: data.len() as u64,
            bytes: 0,
        };
        if data_size.count > 0 {
            self.parallelizer.sink_dcl(data, &self.sinkers).await?;
        }
        Ok((data_size, last_received_position, last_commit_position))
    }

    pub fn fetch_raw(data: &[DtItem]) -> (u64, Option<Position>, Option<Position>) {
        let mut data_count = 0;
        let mut last_received_position = Option::None;
        let mut last_commit_position = Option::None;
        for i in data.iter() {
            match &i.dt_data {
                DtData::Commit { .. } | DtData::Heartbeat {} | DtData::Ddl { .. } => {
                    last_commit_position = Some(i.position.clone());
                    last_received_position = last_commit_position.clone();
                    continue;
                }
                DtData::Begin {} => {
                    continue;
                }

                DtData::Redis { .. } => {
                    last_received_position = Some(i.position.clone());
                    last_commit_position = last_received_position.clone();
                    data_count += 1;
                }

                _ => {
                    last_received_position = Some(i.position.clone());
                    data_count += 1;
                }
            }
        }

        (data_count, last_received_position, last_commit_position)
    }

    fn fetch_dml(mut data: Vec<DtItem>) -> (Vec<RowData>, Option<Position>, Option<Position>) {
        let mut dml_data = Vec::new();
        let mut last_received_position = Option::None;
        let mut last_commit_position = Option::None;
        for i in data.drain(..) {
            match i.dt_data {
                DtData::Commit { .. } | DtData::Heartbeat {} => {
                    last_commit_position = Some(i.position);
                    last_received_position = last_commit_position.clone();
                    continue;
                }

                DtData::Dml { row_data } => {
                    last_received_position = Some(i.position);
                    dml_data.push(row_data);
                }

                _ => {}
            }
        }

        (dml_data, last_received_position, last_commit_position)
    }

    fn fetch_ddl(mut data: Vec<DtItem>) -> (Vec<DdlData>, Option<Position>, Option<Position>) {
        let mut result = Vec::new();
        let mut last_received_position = Option::None;
        let mut last_commit_position = Option::None;
        for i in data.drain(..) {
            match i.dt_data {
                DtData::Commit { .. } | DtData::Heartbeat {} => {
                    last_commit_position = Some(i.position);
                    last_received_position = last_commit_position.clone();
                    continue;
                }

                DtData::Ddl { ddl_data } => {
                    last_commit_position = Some(i.position);
                    last_received_position = last_commit_position.clone();
                    result.push(ddl_data);
                }

                _ => {}
            }
        }

        (result, last_received_position, last_commit_position)
    }

    fn fetch_dcl(mut data: Vec<DtItem>) -> (Vec<DclData>, Option<Position>, Option<Position>) {
        let mut result = Vec::new();
        let mut last_received_position = Option::None;
        let mut last_commit_position = Option::None;
        for i in data.drain(..) {
            match i.dt_data {
                DtData::Commit { .. } | DtData::Heartbeat {} => {
                    last_commit_position = Some(i.position);
                    last_received_position = last_commit_position.clone();
                }

                DtData::Dcl { dcl_data } => {
                    last_commit_position = Some(i.position);
                    last_received_position = last_commit_position.clone();
                    result.push(dcl_data);
                }

                _ => {}
            }
        }

        (result, last_received_position, last_commit_position)
    }

    fn get_sink_method(&self, data: &Vec<DtItem>) -> SinkMethod {
        for i in data {
            match i.dt_data {
                DtData::Struct { .. } => return SinkMethod::Struct,
                DtData::Ddl { .. } => return SinkMethod::Ddl,
                DtData::Dcl { .. } => return SinkMethod::Dcl,
                DtData::Dml { .. } => match self.sinker_config {
                    SinkerConfig::FoxlakePush { .. }
                    | SinkerConfig::FoxlakeMerge { .. }
                    | SinkerConfig::Foxlake { .. }
                    | SinkerConfig::Redis { .. } => return SinkMethod::Raw,
                    _ => return SinkMethod::Dml,
                },
                DtData::Redis { .. } | DtData::Foxlake { .. } => return SinkMethod::Raw,
                DtData::Begin {} | DtData::Commit { .. } | DtData::Heartbeat {} => {
                    continue;
                }
            }
        }
        SinkMethod::Raw
    }

    async fn record_checkpoint(
        &self,
        last_checkpoint_time: Option<Instant>,
        last_received_position: &Position,
        last_commit_position: &Position,
    ) -> Instant {
        if let Some(last) = last_checkpoint_time {
            if last.elapsed().as_secs() < self.checkpoint_interval_secs {
                return last;
            }
        }

        log_position!("current_position | {}", last_received_position.to_string());
        log_position!("checkpoint_position | {}", last_commit_position.to_string());

        // record position for recovery if necessary
        if let Some(handler) = &self.recorder {
            let record_position = if matches!(last_commit_position, Position::None) {
                last_received_position
            } else {
                last_commit_position
            };
            if let Err(e) = handler.record_position(record_position).await {
                log_error!("failed to record position: {}, err: {}", record_position, e);
            }
        }

        if !matches!(last_commit_position, Position::None) {
            self.syncer.lock().await.committed_position = last_commit_position.to_owned();
        }

        self.monitor.set_counter(
            CounterType::Timestamp,
            last_received_position.to_timestamp(),
        );

        Instant::now()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use anyhow::bail;
    use dt_common::{
        error::Error,
        meta::{col_value::ColValue, row_type::RowType},
    };
    use tokio_util::sync::CancellationToken;

    use super::*;

    /// Drains everything queued, then fails the way a broken sinker does.
    struct FailingParallelizer;

    #[async_trait]
    impl Parallelizer for FailingParallelizer {
        fn get_name(&self) -> String {
            "FailingParallelizer".into()
        }

        async fn drain(&mut self, buffer: &DtQueue) -> anyhow::Result<Vec<DtItem>> {
            let mut data = Vec::new();
            while let Ok(item) = buffer.pop().await {
                data.push(item);
            }
            Ok(data)
        }

        async fn sink_dml(
            &mut self,
            _data: Vec<RowData>,
            _sinkers: &[Arc<async_mutex::Mutex<Box<dyn Sinker + Send>>>],
        ) -> anyhow::Result<DataSize> {
            bail!("sinker failed to write the batch")
        }
    }

    fn dml_item() -> DtItem {
        DtItem {
            dt_data: DtData::Dml {
                row_data: RowData::new(
                    "test_db".into(),
                    "test_tb".into(),
                    RowType::Insert,
                    None,
                    Some(HashMap::from([("id".to_string(), ColValue::Long(1))])),
                ),
            },
            position: Position::None,
            data_origin_node: String::new(),
        }
    }

    fn pipeline(buffer: Arc<DtQueue>, cancel_token: CancellationToken) -> BasePipeline {
        BasePipeline {
            buffer,
            parallelizer: Box::new(FailingParallelizer),
            sinker_config: SinkerConfig::Dummy,
            sinkers: Vec::new(),
            cancel_token,
            checkpoint_interval_secs: 3600,
            batch_sink_interval_secs: 0,
            syncer: Arc::new(Mutex::new(Syncer {
                received_position: Position::None,
                committed_position: Position::None,
            })),
            monitor: Arc::new(Monitor::new("pipeline", "test", 1, 100, 1)),
            data_marker: None,
            lua_processor: None,
            recorder: None,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_sinker_failure_releases_an_extractor_blocked_on_a_full_buffer() {
        let cancel_token = CancellationToken::new();
        let buffer = Arc::new(DtQueue::new(1, 0, None, None, cancel_token.clone()));

        // an extractor with more data than the buffer holds: it parks inside push
        let extractor = {
            let buffer = buffer.clone();
            tokio::spawn(async move {
                for _ in 0..64 {
                    buffer.push(dml_item()).await?;
                }
                Ok::<(), anyhow::Error>(())
            })
        };

        let mut pipeline = pipeline(buffer, cancel_token.clone());
        let err = pipeline
            .start()
            .await
            .expect_err("the failing sinker must surface as a pipeline error");
        assert!(err.to_string().contains("sinker failed to write the batch"));

        // this is what the task runner does when either side fails
        cancel_token.cancel();

        let extractor_res = tokio::time::timeout(Duration::from_secs(5), extractor)
            .await
            .expect("the extractor stayed blocked on the buffer after the pipeline died")
            .unwrap();
        let extractor_err = extractor_res.expect_err("a cancelled push must not report success");
        assert!(
            extractor_err
                .chain()
                .any(|cause| matches!(cause.downcast_ref::<Error>(), Some(Error::Cancelled(_)))),
            "expected a cancellation error, got: {:#}",
            extractor_err
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_idle_pipeline_exits_once_the_task_is_cancelled() {
        let cancel_token = CancellationToken::new();
        let buffer = Arc::new(DtQueue::new(8, 0, None, None, cancel_token.clone()));
        let mut pipeline = pipeline(buffer, cancel_token.clone());

        let started = tokio::spawn(async move { pipeline.start().await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !started.is_finished(),
            "an idle pipeline should keep running"
        );

        cancel_token.cancel();
        tokio::time::timeout(Duration::from_secs(5), started)
            .await
            .expect("the pipeline did not converge after cancellation")
            .unwrap()
            .unwrap();
    }
}
