use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use anyhow::{bail, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{base_pipeline::BasePipeline, Pipeline};
use dt_common::{
    log_error, log_info, log_position,
    meta::{
        avro::avro_converter::AvroConverter,
        dt_data::{DtData, DtItem},
        dt_queue::DtQueue,
        position::Position,
        syncer::Syncer,
    },
    monitor::{counter_type::CounterType, monitor::Monitor},
};
use dt_parallelizer::base_parallelizer::BaseParallelizer;

type PositionInfo = (Option<Position>, Option<Position>);

/// How long a shutting-down server waits for the consumer to ack what it already fetched.
/// Without it, the last fetched batch would never be checkpointed.
const ACK_GRACE: Duration = Duration::from_secs(30);
/// Poll interval while waiting out [`ACK_GRACE`].
const ACK_POLL: Duration = Duration::from_millis(200);

#[derive(Clone)]
pub struct HttpServerPipeline {
    pub buffer: Arc<DtQueue>,
    pub syncer: Arc<Mutex<Syncer>>,
    pub monitor: Arc<Monitor>,
    pub avro_converter: AvroConverter,
    pub checkpoint_interval_secs: u64,
    pub batch_sink_interval_secs: u64,
    pub http_host: String,
    pub http_port: u64,
    /// Cancelled when the task is shutting down, whichever side triggered it.
    pub cancel_token: CancellationToken,

    acked_batch_id: Arc<AtomicU64>,
    sent_batch_id: Arc<AtomicU64>,
    pending_ack_data: Arc<async_std::sync::Mutex<HashMap<u64, FetchResp>>>,
    pending_ack_positions: Arc<async_std::sync::Mutex<HashMap<u64, PositionInfo>>>,
    /// Items already popped off the queue for a batch that failed to encode. They are
    /// retried on the next fetch, so a failed response never swallows them.
    carry_over: Arc<async_std::sync::Mutex<Vec<DtItem>>>,
}

#[derive(Deserialize)]
struct FetchNewParams {
    batch_size: usize,
    ack_batch_id: Option<u64>,
}

#[derive(Deserialize)]
struct FetchOldParams {
    old_batch_id: u64,
}

#[derive(Deserialize)]
struct AckReq {
    ack_batch_id: u64,
}

#[derive(Serialize)]
struct AckResp {
    acked_batch_id: u64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct FetchResp {
    data: Vec<Vec<u8>>,
    batch_id: u64,
}

#[derive(Serialize)]
struct InfoResp {
    acked_batch_id: u64,
    sent_batch_id: u64,
}

impl HttpServerPipeline {
    #![allow(clippy::too_many_arguments)]
    pub fn new(
        buffer: Arc<DtQueue>,
        syncer: Arc<Mutex<Syncer>>,
        monitor: Arc<Monitor>,
        avro_converter: AvroConverter,
        checkpoint_interval_secs: u64,
        batch_sink_interval_secs: u64,
        http_host: &str,
        http_port: u64,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            buffer,
            syncer,
            monitor,
            avro_converter,
            checkpoint_interval_secs,
            batch_sink_interval_secs,
            http_host: http_host.into(),
            http_port,
            cancel_token,
            acked_batch_id: Default::default(),
            sent_batch_id: Default::default(),
            pending_ack_data: Default::default(),
            pending_ack_positions: Default::default(),
            carry_over: Default::default(),
        }
    }
}

impl HttpServerPipeline {
    /// Wait until every sent batch has been acked, up to [`ACK_GRACE`]. Anything still
    /// unacked when the window closes is reported: its position is not checkpointed.
    async fn wait_for_pending_acks(&self) {
        let deadline = tokio::time::Instant::now() + ACK_GRACE;
        loop {
            let sent = self.sent_batch_id.load(Ordering::Acquire);
            let acked = self.acked_batch_id.load(Ordering::Acquire);
            if acked >= sent {
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                log_error!(
                    "http server pipeline is stopping with batches [{}..{}] unacked after {}s, their positions are not checkpointed",
                    acked + 1,
                    sent,
                    ACK_GRACE.as_secs()
                );
                return;
            }
            tokio::time::sleep(ACK_POLL).await;
        }
    }
}

#[async_trait]
impl Pipeline for HttpServerPipeline {
    async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn start(&mut self) -> anyhow::Result<()> {
        let app_data = self.clone();
        let bind_addr = format!("{}:{}", self.http_host, self.http_port);
        let server = HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(app_data.clone()))
                .service(web::resource("/info").route(web::get().to(info)))
                .service(web::resource("/fetch_new").route(web::get().to(fetch_new)))
                .service(web::resource("/fetch_old").route(web::get().to(fetch_old)))
                .service(web::resource("/ack").route(web::post().to(ack)))
        })
        .bind(&bind_addr)
        .with_context(|| format!("http server pipeline failed to bind [{}]", bind_addr))?
        .run();

        // spawned, never block_on'd: blocking a runtime worker here would stall every
        // other task sharing it, the extractor included. It also has to outlive the
        // select! below, since stopping it means talking to the running server.
        let handle = server.handle();
        let mut server_task = tokio::spawn(server);

        tokio::select! {
            res = &mut server_task => {
                res.context("http server pipeline panicked")?
                    .context("http server pipeline exited with error")?;
            }
            _ = self.cancel_token.cancelled() => {
                // the consumer acks out of band, so the last fetched batch is only
                // checkpointed if we stay up long enough to receive its ack
                self.wait_for_pending_acks().await;
                log_info!("http server pipeline is shutting down");
                handle.stop(true).await;
                server_task
                    .await
                    .context("http server pipeline panicked while stopping")?
                    .context("http server pipeline failed to stop cleanly")?;
            }
        }
        Ok(())
    }
}

async fn info(pipeline: web::Data<HttpServerPipeline>) -> impl Responder {
    send_response(&InfoResp {
        acked_batch_id: pipeline.acked_batch_id.load(Ordering::Acquire),
        sent_batch_id: pipeline.sent_batch_id.load(Ordering::Acquire),
    })
}

async fn fetch_new(
    query: web::Query<FetchNewParams>,
    pipeline: web::Data<HttpServerPipeline>,
) -> impl Responder {
    if let Some(ack_batch_id) = query.ack_batch_id {
        if let Err(err) = do_ack(ack_batch_id, &pipeline).await {
            return HttpResponse::BadRequest().body(err.to_string());
        }
    }

    let mut pending_ack_data = pipeline.pending_ack_data.lock().await;
    let mut pending_ack_positions = pipeline.pending_ack_positions.lock().await;
    let sent_batch_id = pipeline.sent_batch_id.load(Ordering::Acquire);

    // get data from buffer
    let mut parallelizer = BaseParallelizer {
        monitor: pipeline.monitor.clone(),
        ..Default::default()
    };
    // items from a batch that failed to encode come first, so a failed response
    // never loses them and never reorders the stream
    let mut carry_over = pipeline.carry_over.lock().await;
    let mut data = std::mem::take(&mut *carry_over);
    if data.len() < query.batch_size {
        match parallelizer
            .drain_by_count(&pipeline.buffer, query.batch_size - data.len())
            .await
        {
            Ok(drained) => data.extend(drained),
            Err(err) => {
                log_error!("fetch_new failed to drain the buffer, error: {}", err);
                *carry_over = data;
                return HttpResponse::InternalServerError().body(err.to_string());
            }
        }
    }
    let (_, last_received_position, last_commit_position) = BasePipeline::fetch_raw(&data);

    // data -> avro response
    let mut response = FetchResp {
        batch_id: sent_batch_id + 1,
        data: Vec::new(),
    };

    // the converter consumes what it encodes, so encode from a clone: a failure must be
    // able to hand the whole batch back to carry_over intact
    let mut avro_converter = pipeline.avro_converter.clone();
    let mut encode_err = None;
    for i in data.iter() {
        let encoded = match &i.dt_data {
            DtData::Dml { row_data } => {
                avro_converter
                    .row_data_to_avro_value(row_data.clone())
                    .await
            }
            DtData::Ddl { ddl_data } => {
                avro_converter
                    .ddl_data_to_avro_value(ddl_data.clone())
                    .await
            }
            _ => continue,
        };
        match encoded {
            Ok(payload) => response.data.push(payload),
            Err(err) => {
                encode_err = Some(err);
                break;
            }
        }
    }
    if let Some(err) = encode_err {
        log_error!(
            "fetch_new failed to encode data, the batch is kept for the next fetch, error: {}",
            err
        );
        *carry_over = data;
        return HttpResponse::InternalServerError().body(err.to_string());
    }

    // update monitor
    pipeline
        .monitor
        .add_counter(CounterType::BufferSize, pipeline.buffer.len() as u64)
        .await
        .add_counter(CounterType::SinkedRecordTotal, response.data.len() as u64)
        .await;

    // update pending_ack_data & pending_ack_positions
    let batch_id = response.batch_id;
    pipeline.sent_batch_id.store(batch_id, Ordering::Release);
    if !response.data.is_empty() {
        pending_ack_positions.insert(batch_id, (last_received_position, last_commit_position));
        let stored = pending_ack_data.entry(batch_id).or_insert(response);
        send_response(stored)
    } else {
        send_response(&response)
    }
}

async fn fetch_old(
    query: web::Query<FetchOldParams>,
    pipeline: web::Data<HttpServerPipeline>,
) -> impl Responder {
    let acked_batch_id = pipeline.acked_batch_id.load(Ordering::Acquire);
    let sent_batch_id = pipeline.sent_batch_id.load(Ordering::Acquire);
    let old_batch_id = query.old_batch_id;

    if old_batch_id > sent_batch_id {
        return HttpResponse::BadRequest().body(format!(
            "old_batch_id: [{}] must <= sent_batch_id: [{}]",
            old_batch_id, sent_batch_id
        ));
    }

    if old_batch_id <= acked_batch_id {
        return HttpResponse::BadRequest().body(format!(
            "old_batch_id: [{}] must > acked_batch_id: [{}]",
            old_batch_id, acked_batch_id
        ));
    }

    if let Some(response) = pipeline.pending_ack_data.lock().await.get(&old_batch_id) {
        send_response(response)
    } else {
        // should never happen
        send_response(&FetchResp::default())
    }
}

async fn ack(data: web::Json<AckReq>, pipeline: web::Data<HttpServerPipeline>) -> impl Responder {
    if let Err(err) = do_ack(data.ack_batch_id, &pipeline).await {
        return HttpResponse::BadRequest().body(err.to_string());
    }
    send_response(&AckResp {
        acked_batch_id: pipeline.acked_batch_id.load(Ordering::Acquire),
    })
}

fn send_response<T: Serialize>(response: &T) -> HttpResponse {
    match serde_json::to_string(response) {
        Ok(json) => HttpResponse::Ok()
            .content_type("application/json")
            .body(json),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

async fn do_ack(ack_batch_id: u64, pipeline: &web::Data<HttpServerPipeline>) -> anyhow::Result<()> {
    let acked_batch_id = pipeline.acked_batch_id.load(Ordering::Acquire);
    let sent_batch_id = pipeline.sent_batch_id.load(Ordering::Acquire);

    if ack_batch_id > sent_batch_id {
        bail!(format!(
            "ack_batch_id: [{}] must <= sent_batch_id: [{}]",
            ack_batch_id, sent_batch_id
        ));
    }

    if ack_batch_id < acked_batch_id {
        bail!(format!(
            "ack_batch_id: [{}] must >= acked_batch_id : [{}]",
            ack_batch_id, acked_batch_id
        ));
    }

    let mut pending_ack_data = pipeline.pending_ack_data.lock().await;
    refresh_appending_ack_data(&mut pending_ack_data, ack_batch_id);

    let mut pending_ack_positions = pipeline.pending_ack_positions.lock().await;
    let max_acked_position_info =
        refresh_appending_ack_positions(&mut pending_ack_positions, ack_batch_id);

    record_checkpoint(max_acked_position_info);
    pipeline
        .acked_batch_id
        .store(ack_batch_id, Ordering::Release);
    Ok(())
}

fn refresh_appending_ack_data(
    pending_ack_data: &mut async_std::sync::MutexGuard<'_, HashMap<u64, FetchResp>>,
    ack_batch_id: u64,
) {
    pending_ack_data.retain(|&batch_id, _| batch_id > ack_batch_id);
}

fn refresh_appending_ack_positions(
    pending_ack_positions: &mut async_std::sync::MutexGuard<'_, HashMap<u64, PositionInfo>>,
    ack_batch_id: u64,
) -> PositionInfo {
    let mut max_acked_batch_id = 0;
    let mut max_acked_position_info = (None, None);
    for (batch_id, position_info) in pending_ack_positions.iter() {
        if *batch_id <= ack_batch_id && *batch_id >= max_acked_batch_id {
            max_acked_batch_id = *batch_id;
            if let Some(last_received_position) = &position_info.0 {
                max_acked_position_info.0 = Some(last_received_position.to_owned());
            }
            if let Some(last_commit_position) = &position_info.1 {
                max_acked_position_info.1 = Some(last_commit_position.to_owned());
            }
        }
    }
    pending_ack_positions.retain(|&batch_id, _| batch_id > ack_batch_id);
    max_acked_position_info
}

fn record_checkpoint(position_info: PositionInfo) {
    if let Some(current_position) = position_info.0 {
        log_position!("current_position | {}", current_position.to_string());
    }
    if let Some(checkpoint_position) = position_info.1 {
        log_position!("checkpoint_position | {}", checkpoint_position.to_string());
    }
}
