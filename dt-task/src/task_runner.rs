use std::{collections::VecDeque, panic, sync::Arc};

use anyhow::{bail, Context};
use chrono::Local;
use log4rs::config::{Config, Deserializers, RawConfig};
use tokio::{
    fs::{metadata, File},
    io::AsyncReadExt,
    select,
    sync::{Mutex, RwLock},
    task::JoinSet,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

use super::{
    extractor_util::{ExtractorUtil, PartitionCols},
    parallelizer_util::ParallelizerUtil,
    sinker_util::SinkerUtil,
};
use crate::task_util::{ConnClient, TaskUtil};
use async_mutex::Mutex as AsyncMutex;
use std::sync::Mutex as StdMutex;

static LOG_HANDLE: StdMutex<Option<log4rs::Handle>> = StdMutex::new(None);
use dt_common::log_filter::SizeLimitFilterDeserializer;
use dt_common::{
    config::{
        config_enums::{build_task_type, DbType, PipelineType, TaskType},
        config_token_parser::{ConfigTokenParser, TokenEscapePair},
        extractor_config::ExtractorConfig,
        sinker_config::SinkerConfig,
        task_config::{TaskConfig, DEFAULT_CHECK_LOG_FILE_SIZE},
    },
    error::Error,
    limiter::buffer_limiter::BufferLimiter,
    log_error, log_finished, log_info, log_warn,
    meta::{
        avro::avro_converter::AvroConverter, dt_queue::DtQueue, position::Position,
        row_type::RowType, syncer::Syncer,
    },
    monitor::{
        group_monitor::GroupMonitor,
        monitor::Monitor,
        task_metrics::TaskMetricsType,
        task_monitor::{MonitorType, TaskMonitor},
        FlushableMonitor,
    },
    rdb_filter::RdbFilter,
    utils::sql_util::SqlUtil,
};
use dt_connector::{
    check_log::check_log::CheckSummaryLog,
    data_marker::DataMarker,
    extractor::resumer::{recorder::Recorder, recovery::Recovery},
    rdb_router::RdbRouter,
    Sinker,
};
use dt_pipeline::{
    base_pipeline::BasePipeline, http_server_pipeline::HttpServerPipeline,
    lua_processor::LuaProcessor, Pipeline,
};

#[cfg(feature = "metrics")]
use dt_common::monitor::prometheus_metrics::PrometheusMetrics;

#[derive(Clone)]
pub struct TaskContext {
    pub id: String,
    pub extractor_config: ExtractorConfig,
    pub extractor_client: ConnClient,
    pub partition_cols: Option<Arc<PartitionCols>>,
    pub sinker_client: ConnClient,
    pub router: Arc<RdbRouter>,
    pub recorder: Option<Arc<dyn Recorder + Send + Sync>>,
    pub recovery: Option<Arc<dyn Recovery + Send + Sync>>,
    pub check_summary: Option<Arc<AsyncMutex<CheckSummaryLog>>>,
    pub enqueue_limiter: Option<Arc<BufferLimiter>>,
    pub dequeue_limiter: Option<Arc<BufferLimiter>>,
    /// Parent of every token below it: cancelling it stops the whole task tree.
    pub cancel_token: CancellationToken,
}

#[derive(Clone)]
pub struct TaskRunner {
    task_type: Option<TaskType>,
    config: TaskConfig,
    extractor_monitor: Arc<GroupMonitor>,
    pipeline_monitor: Arc<GroupMonitor>,
    sinker_monitor: Arc<GroupMonitor>,
    task_monitor: Arc<TaskMonitor>,
    #[cfg(feature = "metrics")]
    prometheus_metrics: Arc<PrometheusMetrics>,
}

const CHECK_LOG_DIR_PLACEHOLDER: &str = "CHECK_LOG_DIR_PLACEHOLDER";
const STATISTIC_LOG_DIR_PLACEHOLDER: &str = "STATISTIC_LOG_DIR_PLACEHOLDER";
const LOG_LEVEL_PLACEHOLDER: &str = "LOG_LEVEL_PLACEHOLDER";
const LOG_DIR_PLACEHOLDER: &str = "LOG_DIR_PLACEHOLDER";
const CHECK_LOG_FILE_SIZE_PLACEHOLDER: &str = "CHECK_LOG_FILE_SIZE_PLACEHOLDER";
const DEFAULT_CHECK_LOG_DIR_PLACEHOLDER: &str = "LOG_DIR_PLACEHOLDER/check";
const DEFAULT_STATISTIC_LOG_DIR_PLACEHOLDER: &str = "LOG_DIR_PLACEHOLDER/statistic";
/// How long sibling sub tasks get to converge after one of them failed, before they are aborted.
const MULTI_TASK_CONVERGE_TIMEOUT: Duration = Duration::from_secs(30);

/// Tokio task cancellation only aborts the current future. JoinHandles spawned inside (extractor,
/// pipeline, monitors, etc.) will keep running unless we explicitly stop them.
///
/// This guard ensures that dropping/aborting a `TaskRunner::start_task()` future triggers a best-
/// effort shutdown of the internal tasks to avoid leaking long-running CDC jobs (replication slot
/// stays active) across retries in integration tests.
struct AbortGuard {
    cancel_token: CancellationToken,
    abort_handles: Vec<tokio::task::AbortHandle>,
    armed: bool,
}

impl AbortGuard {
    fn new(cancel_token: CancellationToken, abort_handles: Vec<tokio::task::AbortHandle>) -> Self {
        Self {
            cancel_token,
            abort_handles,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for AbortGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        // Allow cooperative exit first, then force-abort if the inner tasks are stuck.
        let first = !self.cancel_token.is_cancelled();
        self.cancel_token.cancel();
        if first {
            log_warn!(
                "shutdown triggered by AbortGuard drop (start_task future cancelled/dropped). Aborting {} inner tasks.",
                self.abort_handles.len()
            );
        }
        for h in &self.abort_handles {
            h.abort();
        }
    }
}

impl TaskRunner {
    pub fn new(task_config_file: &str) -> anyhow::Result<Self> {
        let config = TaskConfig::new(task_config_file)
            .with_context(|| format!("invalid configs in [{}]", task_config_file))?;
        let task_type = build_task_type(
            &config.extractor_basic.extract_type,
            &config.sinker_basic.sink_type,
        );
        #[cfg(not(feature = "metrics"))]
        let task_monitor = Arc::new(TaskMonitor::new(task_type.clone()));

        #[cfg(feature = "metrics")]
        let prometheus_metrics = Arc::new(PrometheusMetrics::new(
            task_type.clone(),
            config.metrics.clone(),
        ));

        #[cfg(feature = "metrics")]
        let task_monitor = Arc::new(TaskMonitor::new(
            task_type.clone(),
            prometheus_metrics.clone(),
        ));

        Ok(Self {
            config,
            extractor_monitor: Arc::new(GroupMonitor::new("extractor", "global")),
            pipeline_monitor: Arc::new(GroupMonitor::new("pipeline", "global")),
            sinker_monitor: Arc::new(GroupMonitor::new("sinker", "global")),
            task_monitor,
            #[cfg(feature = "metrics")]
            prometheus_metrics,
            task_type,
        })
    }

    pub async fn start_task(&self) -> anyhow::Result<()> {
        self.init_log4rs().await?;

        panic::set_hook(Box::new(|panic_info| {
            let backtrace = std::backtrace::Backtrace::capture();
            log_error!("panic: {}\nbacktrace:\n{}", panic_info, backtrace);
        }));

        log_info!(
            "start task: [taskID: {}, taskType: {:?}]",
            &self.config.global.task_id,
            &self.task_type
        );

        let db_type = &self.config.extractor_basic.db_type;
        let router = Arc::new(RdbRouter::from_config(&self.config.router, db_type)?);
        let (recorder, recovery) = match &self.task_type {
            Some(task_type) => {
                TaskUtil::build_resumer(
                    task_type.to_owned(),
                    &self.config.global,
                    &self.config.resumer,
                )
                .await?
            }
            None => (None, None),
        };
        let (extractor_client, sinker_client) = ConnClient::from_config(&self.config).await?;
        let enqueue_limiter = BufferLimiter::from_config(
            Some(&self.config.extractor_basic.rate_limiter),
            Some(&self.config.pipeline.capacity_limiter),
        )
        .map(Arc::new);
        let dequeue_limiter =
            BufferLimiter::from_config(Some(&self.config.sinker_basic.rate_limiter), None)
                .map(Arc::new);

        let check_summary = match &self.config.sinker {
            SinkerConfig::MysqlCheck { .. }
            | SinkerConfig::PgCheck { .. }
            | SinkerConfig::MongoCheck { .. } => Some(Arc::new(AsyncMutex::new(CheckSummaryLog {
                start_time: Local::now().to_rfc3339(),
                ..Default::default()
            }))),
            _ => None,
        };

        let partition_cols = match &self.config.extractor {
            ExtractorConfig::MysqlSnapshot { partition_cols, .. }
            | ExtractorConfig::PgSnapshot { partition_cols, .. }
            | ExtractorConfig::OracleSnapshot { partition_cols, .. } => Some(Arc::new(
                ExtractorUtil::parse_partition_cols(partition_cols)?,
            )),
            _ => None,
        };

        let task_context = TaskContext {
            id: String::new(),
            extractor_config: self.config.extractor.clone(),
            extractor_client: extractor_client.clone(),
            partition_cols,
            sinker_client: sinker_client.clone(),
            router,
            recorder,
            recovery,
            check_summary: check_summary.clone(),
            enqueue_limiter,
            dequeue_limiter,
            cancel_token: CancellationToken::new(),
        };

        #[cfg(feature = "metrics")]
        self.prometheus_metrics
            .initialization()
            .start_metrics()
            .await;

        match &self.config.extractor {
            ExtractorConfig::MysqlStruct { .. }
            | ExtractorConfig::PgStruct { .. }
            | ExtractorConfig::OracleStruct { .. } => {
                let mut pending_tasks = self.build_pending_tasks(task_context, false).await?;
                if let Some(task_context) = pending_tasks.pop_front() {
                    self.clone().start_single_task(task_context, false).await?
                }
            }

            ExtractorConfig::MysqlSnapshot { .. }
            | ExtractorConfig::PgSnapshot { .. }
            | ExtractorConfig::OracleSnapshot { .. }
            | ExtractorConfig::MongoSnapshot { .. }
            | ExtractorConfig::FoxlakeS3 { .. } => self.start_multi_task(task_context).await?,

            _ => self.clone().start_single_task(task_context, false).await?,
        };

        // close connections
        extractor_client.close().await?;
        sinker_client.close().await?;

        if let Some(check_summary) = check_summary {
            let summary = check_summary.lock().await;
            if summary.miss_count > 0 || summary.diff_count > 0 || summary.extra_count > 0 {
                dt_common::log_summary!("{}", summary);
            }
        }

        log_finished!("task finished");
        Ok(())
    }

    async fn start_multi_task(&self, task_context: TaskContext) -> anyhow::Result<()> {
        let root_cancel_token = task_context.cancel_token.clone();
        let mut pending_tasks = self.build_pending_tasks(task_context, true).await?;

        // start a thread to flush global monitors
        let global_cancel_token = CancellationToken::new();
        let global_cancel_token_clone = global_cancel_token.clone();
        let interval_secs = self.config.pipeline.checkpoint_interval_secs;
        let extractor_monitor = self.extractor_monitor.clone();
        let pipeline_monitor = self.pipeline_monitor.clone();
        let sinker_monitor = self.sinker_monitor.clone();
        let task_monitor = self.task_monitor.clone();
        let global_monitor_task = tokio::spawn(async move {
            Self::flush_monitors_generic::<GroupMonitor, TaskMonitor>(
                interval_secs,
                global_cancel_token_clone,
                &[extractor_monitor, pipeline_monitor, sinker_monitor],
                &[task_monitor],
            )
            .await
        });
        let mut global_abort_guard = AbortGuard::new(
            global_cancel_token.clone(),
            vec![global_monitor_task.abort_handle()],
        );

        let task_parallel_size = self.get_task_parallel_size();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(task_parallel_size));
        let mut join_set: JoinSet<(String, anyhow::Result<()>)> = JoinSet::new();

        // initialize the task pool to its maximum capacity
        while join_set.len() < task_parallel_size && !pending_tasks.is_empty() {
            if let Some(task_context) = pending_tasks.pop_front() {
                self.clone()
                    .spawn_single_task(task_context, &mut join_set, &semaphore)
                    .await?;
            }
        }

        // when a task is completed, if there are still pending tables, add a new task
        let mut errors: Vec<String> = Vec::new();
        while let Some(result) = join_set.join_next().await {
            Self::collect_single_task_result(result, &mut errors);
            if !errors.is_empty() {
                break;
            }
            if let Some(task_context) = pending_tasks.pop_front() {
                self.clone()
                    .spawn_single_task(task_context, &mut join_set, &semaphore)
                    .await?;
            }
        }

        if !errors.is_empty() {
            // do not abandon the sibling tasks: aborting them mid-write would drop
            // in-flight batches without recording their positions. Cancel, then give
            // them a bounded window to converge, and only abort what is still stuck.
            log_error!(
                "single task failed, cancelling {} sibling task(s): {}",
                join_set.len(),
                errors[0]
            );
            pending_tasks.clear();
            root_cancel_token.cancel();
            let deadline = tokio::time::Instant::now() + MULTI_TASK_CONVERGE_TIMEOUT;
            while !join_set.is_empty() {
                match tokio::time::timeout_at(deadline, join_set.join_next()).await {
                    Ok(Some(result)) => Self::collect_single_task_result(result, &mut errors),
                    Ok(None) => break,
                    Err(_) => {
                        log_error!(
                            "{} sibling task(s) did not converge within {}s, aborting them",
                            join_set.len(),
                            MULTI_TASK_CONVERGE_TIMEOUT.as_secs()
                        );
                        errors.push(format!(
                            "{} sibling task(s) had to be aborted after failing to converge within {}s",
                            join_set.len(),
                            MULTI_TASK_CONVERGE_TIMEOUT.as_secs()
                        ));
                        join_set.shutdown().await;
                        break;
                    }
                }
            }
        }

        global_cancel_token.cancel();
        global_monitor_task.await?;
        global_abort_guard.disarm();

        if !errors.is_empty() {
            bail!("multi task failed:\n  {}", errors.join("\n  "));
        }
        Ok(())
    }

    /// Sort one finished sub task into "keep going" or "record the error"; a task aborted
    /// during convergence is expected noise, not a new failure.
    fn collect_single_task_result(
        result: Result<(String, anyhow::Result<()>), tokio::task::JoinError>,
        errors: &mut Vec<String>,
    ) {
        match result {
            Ok((_, Ok(()))) => {}
            Ok((single_task_id, Err(e))) => {
                errors.push(format!("single task: [{}] failed, error: {:#}", single_task_id, e))
            }
            Err(e) if e.is_cancelled() => {}
            Err(e) => errors.push(format!("join error: {}", e)),
        }
    }

    async fn spawn_single_task(
        self,
        task_context: TaskContext,
        join_set: &mut JoinSet<(String, anyhow::Result<()>)>,
        semaphore: &Arc<tokio::sync::Semaphore>,
    ) -> anyhow::Result<()> {
        let single_task_id = task_context.id;
        let semaphore = Arc::clone(semaphore);
        let task_context = TaskContext {
            id: single_task_id.clone(),
            extractor_config: task_context.extractor_config,
            extractor_client: task_context.extractor_client,
            sinker_client: task_context.sinker_client,
            router: task_context.router,
            recorder: task_context.recorder,
            recovery: task_context.recovery,
            check_summary: task_context.check_summary,
            partition_cols: task_context.partition_cols,
            enqueue_limiter: task_context.enqueue_limiter,
            dequeue_limiter: task_context.dequeue_limiter,
            cancel_token: task_context.cancel_token,
        };
        let me = self.clone();
        join_set.spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            let res = me.start_single_task(task_context, true).await;
            (single_task_id, res)
        });
        Ok(())
    }

    async fn start_single_task(
        self,
        task_context: TaskContext,
        is_multi_task: bool,
    ) -> anyhow::Result<()> {
        let extractor_config = task_context.extractor_config;
        let extractor_client = task_context.extractor_client;
        let sinker_client = task_context.sinker_client;
        let router = (*task_context.router).clone();
        let recorder = task_context.recorder.clone();
        let recovery = task_context.recovery.clone();
        let enqueue_limiter = task_context.enqueue_limiter;
        let dequeue_limiter = task_context.dequeue_limiter;

        // a child of the task-wide token: a failure here cancels only this sub task,
        // while a task-wide cancel still reaches every side of it
        let cancel_token = task_context.cancel_token.child_token();

        let max_bytes = self.config.pipeline.capacity_limiter.buffer_memory_mb * 1024 * 1024;
        let buffer = Arc::new(DtQueue::new(
            self.config.pipeline.capacity_limiter.buffer_size,
            max_bytes as u64,
            enqueue_limiter,
            dequeue_limiter,
            cancel_token.clone(),
        ));

        let syncer = Arc::new(Mutex::new(Syncer {
            received_position: Position::None,
            committed_position: Position::None,
        }));

        let (extractor_data_marker, sinker_data_marker) = if let Some(data_marker_config) =
            &self.config.data_marker
        {
            let extractor_data_marker =
                DataMarker::from_config(data_marker_config, &self.config.extractor_basic.db_type)?;
            let sinker_data_marker =
                DataMarker::from_config(data_marker_config, &self.config.sinker_basic.db_type)?;
            (Some(extractor_data_marker), Some(sinker_data_marker))
        } else {
            (None, None)
        };
        let rw_sinker_data_marker = sinker_data_marker
            .clone()
            .map(|data_marker| Arc::new(RwLock::new(data_marker)));

        let single_task_id = match &extractor_config {
            ExtractorConfig::MysqlSnapshot { db, tb, .. } => format!("{}.{}", db, tb),
            ExtractorConfig::PgSnapshot { schema, tb, .. } => format!("{}.{}", schema, tb),
            ExtractorConfig::MongoSnapshot { db, tb, .. } => format!("{}.{}", db, tb),
            _ => String::new(),
        };

        // extractor
        let monitor_time_window_secs = self.config.pipeline.counter_time_window_secs;
        let monitor_max_sub_count = self.config.pipeline.counter_max_sub_count;
        let monitor_count_window = self.config.pipeline.capacity_limiter.buffer_size as u64;
        let extractor_monitor = Arc::new(Monitor::new(
            "extractor",
            &single_task_id,
            monitor_time_window_secs,
            monitor_max_sub_count,
            monitor_count_window,
        ));
        let mut extractor = ExtractorUtil::create_extractor(
            &self.config,
            &extractor_config,
            extractor_client.clone(),
            task_context.partition_cols,
            buffer.clone(),
            cancel_token.clone(),
            syncer.clone(),
            extractor_monitor.clone(),
            extractor_data_marker,
            router,
            recovery,
        )
        .await?;

        // sinkers
        let sinker_monitor = Arc::new(Monitor::new(
            "sinker",
            &single_task_id,
            monitor_time_window_secs,
            monitor_max_sub_count,
            monitor_count_window,
        ));
        let sinkers = SinkerUtil::create_sinkers(
            &self.config,
            &extractor_config,
            sinker_client.clone(),
            sinker_monitor.clone(),
            rw_sinker_data_marker.clone(),
            task_context.check_summary.clone(),
        )
        .await?;

        // pipeline
        let pipeline_monitor = Arc::new(Monitor::new(
            "pipeline",
            &single_task_id,
            monitor_time_window_secs,
            monitor_max_sub_count,
            monitor_count_window,
        ));

        let mut pipeline = self
            .create_pipeline(
                buffer,
                cancel_token.clone(),
                syncer,
                sinkers,
                pipeline_monitor.clone(),
                rw_sinker_data_marker.clone(),
                recorder.clone(),
            )
            .await?;

        // add monitors to global monitors
        tokio::join!(
            async {
                self.extractor_monitor
                    .add_monitor(&single_task_id, extractor_monitor.clone());
            },
            async {
                self.pipeline_monitor
                    .add_monitor(&single_task_id, pipeline_monitor.clone());
            },
            async {
                self.sinker_monitor
                    .add_monitor(&single_task_id, sinker_monitor.clone());
            },
            async {
                self.task_monitor.register(
                    &single_task_id,
                    vec![
                        (MonitorType::Extractor, extractor_monitor.clone()),
                        (MonitorType::Pipeline, pipeline_monitor.clone()),
                        (MonitorType::Sinker, sinker_monitor.clone()),
                    ],
                );
            }
        );

        // do pre operations before task starts
        self.pre_single_task(
            extractor_client.clone(),
            sinker_client.clone(),
            sinker_data_marker,
        )
        .await?;

        // start threads (avoid unwrap-panics; propagate errors with context)
        // If either side errors, cancel the token so the other side can exit and avoid deadlock.
        let cancel_token_for_extractor = cancel_token.clone();
        let f1 = tokio::spawn(async move {
            let extract_res = extractor.extract().await;
            let close_res = extractor.close().await;
            if extract_res.is_err() || close_res.is_err() {
                let first = !cancel_token_for_extractor.is_cancelled();
                cancel_token_for_extractor.cancel();
                if first {
                    log_error!("shutdown triggered by extractor error; forcing pipeline shutdown");
                }
            }
            if let Err(e) = extract_res {
                return Err(e).context("extractor.extract failed");
            }
            if let Err(e) = close_res {
                return Err(e).context("extractor.close failed");
            }
            Ok::<(), anyhow::Error>(())
        });

        let cancel_token_for_pipeline = cancel_token.clone();
        let f2 = tokio::spawn(async move {
            let start_res = pipeline.start().await;
            let stop_res = pipeline.stop().await;
            if start_res.is_err() || stop_res.is_err() {
                let first = !cancel_token_for_pipeline.is_cancelled();
                cancel_token_for_pipeline.cancel();
                if first {
                    log_error!("shutdown triggered by pipeline error; forcing extractor shutdown");
                }
                if let Err(e) = &start_res {
                    log_error!("pipeline.start returned error: {e:#}");
                }
                if let Err(e) = &stop_res {
                    log_error!("pipeline.stop returned error: {e:#}");
                }
            }
            if let Err(e) = start_res {
                return Err(e).context("pipeline.start failed");
            }
            if let Err(e) = stop_res {
                return Err(e).context("pipeline.stop failed");
            }
            Ok::<(), anyhow::Error>(())
        });

        let interval_secs = self.config.pipeline.checkpoint_interval_secs;
        let tasks: Vec<Arc<TaskMonitor>> = if is_multi_task {
            vec![]
        } else {
            vec![self.task_monitor.clone()]
        };
        let cancel_token_for_monitors = cancel_token.clone();
        let f3 = tokio::spawn(async move {
            Self::flush_monitors_generic::<Monitor, TaskMonitor>(
                interval_secs,
                cancel_token_for_monitors,
                &[extractor_monitor, pipeline_monitor, sinker_monitor],
                &tasks,
            )
            .await
        });

        let mut abort_guard = AbortGuard::new(
            cancel_token.clone(),
            vec![f1.abort_handle(), f2.abort_handle(), f3.abort_handle()],
        );
        // JoinHandle<T>::await -> Result<T, JoinError>
        // Here f1/f2 return anyhow::Result<()>, so we need to unwrap twice.
        let (r1, r2, r3) = tokio::join!(f1, f2, f3);
        abort_guard.disarm();
        // the pipeline is reported first: when it dies, the extractor's own error is
        // usually just the cancellation that unblocked it, not the root cause
        Self::report_task_results(vec![("pipeline", r2), ("extractor", r1)])?;
        r3?;

        // Post-drain flush: the pipeline may have processed additional rows
        // after f3 (monitor flush) exited on cancellation.  Recalculate so that
        // the final Prometheus scrape sees up-to-date sinker counts and
        // progress close to 100% for snapshot tasks.
        {
            let _ = self.task_monitor.calc().await;
            // Brief pause so the Console scraper can observe the updated
            // Prometheus gauges before the process exits.
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }

        // finished log
        let (schema, tb) = match &extractor_config {
            ExtractorConfig::MysqlSnapshot { db, tb, .. }
            | ExtractorConfig::MongoSnapshot { db, tb, .. } => (db.to_owned(), tb.to_owned()),
            ExtractorConfig::PgSnapshot { schema, tb, .. }
            | ExtractorConfig::FoxlakeS3 { schema, tb, .. } => (schema.to_owned(), tb.to_owned()),
            _ => (String::new(), String::new()),
        };
        if !tb.is_empty() {
            let finish_position = Position::RdbSnapshotFinished {
                db_type: self.config.extractor_basic.db_type.to_string(),
                schema,
                tb,
            };
            log_finished!("{}", finish_position.to_string());
            self.task_monitor
                .add_no_window_metrics(TaskMetricsType::FinishedProgressCount, 1);

            if let Some(handler) = &recorder {
                if let Err(e) = handler.record_position(&finish_position).await {
                    log_error!("failed to record position: {}, err: {}", finish_position, e);
                }
            }
        }

        // remove monitors from global monitors
        tokio::join!(
            async {
                self.extractor_monitor.remove_monitor(&single_task_id);
            },
            async {
                self.pipeline_monitor.remove_monitor(&single_task_id);
            },
            async {
                self.sinker_monitor.remove_monitor(&single_task_id);
            },
            async {
                self.task_monitor.unregister(
                    &single_task_id,
                    vec![
                        MonitorType::Extractor,
                        MonitorType::Pipeline,
                        MonitorType::Sinker,
                    ],
                );
            }
        );

        Ok(())
    }

    async fn create_pipeline(
        &self,
        buffer: Arc<DtQueue>,
        cancel_token: CancellationToken,
        syncer: Arc<Mutex<Syncer>>,
        sinkers: Vec<Arc<AsyncMutex<Box<dyn Sinker + Send>>>>,
        monitor: Arc<Monitor>,
        data_marker: Option<Arc<RwLock<DataMarker>>>,
        recorder: Option<Arc<dyn Recorder + Send + Sync>>,
    ) -> anyhow::Result<Box<dyn Pipeline + Send>> {
        match self.config.pipeline.pipeline_type {
            PipelineType::Basic => {
                let lua_processor =
                    self.config
                        .processor
                        .as_ref()
                        .map(|processor_config| LuaProcessor {
                            lua_code: processor_config.lua_code.clone(),
                        });

                let parallelizer =
                    ParallelizerUtil::create_parallelizer(&self.config, monitor.clone()).await?;

                let pipeline = BasePipeline {
                    buffer,
                    parallelizer,
                    sinker_config: self.config.sinker.clone(),
                    sinkers,
                    cancel_token,
                    checkpoint_interval_secs: self.config.pipeline.checkpoint_interval_secs,
                    batch_sink_interval_secs: self.config.pipeline.batch_sink_interval_secs,
                    syncer,
                    monitor,
                    data_marker,
                    lua_processor,
                    recorder,
                };
                Ok(Box::new(pipeline) as Box<dyn Pipeline + Send>)
            }

            PipelineType::HttpServer => {
                let meta_manager = ExtractorUtil::get_extractor_meta_manager(&self.config).await?;
                let avro_converter =
                    AvroConverter::new(meta_manager, self.config.pipeline.with_field_defs);
                let pipeline = HttpServerPipeline::new(
                    buffer,
                    syncer,
                    monitor,
                    avro_converter,
                    self.config.pipeline.checkpoint_interval_secs,
                    self.config.pipeline.batch_sink_interval_secs,
                    &self.config.pipeline.http_host,
                    self.config.pipeline.http_port,
                    cancel_token,
                );
                Ok(Box::new(pipeline) as Box<dyn Pipeline + Send>)
            }
        }
    }

    async fn init_log4rs(&self) -> anyhow::Result<()> {
        let log4rs_file = &self.config.runtime.log4rs_file;
        if metadata(log4rs_file).await.is_err() {
            return Ok(());
        }

        let mut config_str = String::new();
        let mut file = File::open(log4rs_file).await?;
        file.read_to_string(&mut config_str).await?;

        match &self.config.sinker {
            SinkerConfig::MysqlCheck {
                check_log_dir,
                check_log_file_size,
                ..
            }
            | SinkerConfig::PgCheck {
                check_log_dir,
                check_log_file_size,
                ..
            }
            | SinkerConfig::OracleCheck {
                check_log_dir,
                check_log_file_size,
                ..
            }
            | SinkerConfig::MongoCheck {
                check_log_dir,
                check_log_file_size,
                ..
            } => {
                if !check_log_dir.is_empty() {
                    config_str = config_str.replace(CHECK_LOG_DIR_PLACEHOLDER, check_log_dir);
                }
                config_str =
                    config_str.replace(CHECK_LOG_FILE_SIZE_PLACEHOLDER, check_log_file_size);
            }

            SinkerConfig::RedisStatistic {
                statistic_log_dir, ..
            } => {
                if !statistic_log_dir.is_empty() {
                    config_str =
                        config_str.replace(STATISTIC_LOG_DIR_PLACEHOLDER, statistic_log_dir);
                }
            }

            _ => {}
        }

        config_str = config_str
            .replace(CHECK_LOG_DIR_PLACEHOLDER, DEFAULT_CHECK_LOG_DIR_PLACEHOLDER)
            .replace(
                STATISTIC_LOG_DIR_PLACEHOLDER,
                DEFAULT_STATISTIC_LOG_DIR_PLACEHOLDER,
            )
            .replace(CHECK_LOG_FILE_SIZE_PLACEHOLDER, DEFAULT_CHECK_LOG_FILE_SIZE)
            .replace(LOG_DIR_PLACEHOLDER, &self.config.runtime.log_dir)
            .replace(LOG_LEVEL_PLACEHOLDER, &self.config.runtime.log_level);

        let raw: RawConfig = serde_yaml::from_str(&config_str)?;
        let mut deserializers = Deserializers::default();
        deserializers.insert("size_limit", SizeLimitFilterDeserializer);
        let (appenders, errors) = raw.appenders_lossy(&deserializers);
        if !errors.is_empty() {
            bail!("errors deserializing appenders: {:?}", errors);
        }

        let config = Config::builder()
            .appenders(appenders)
            .loggers(raw.loggers())
            .build(raw.root())?;
        let mut handle_guard = LOG_HANDLE.lock().unwrap();
        if let Some(handle) = handle_guard.as_ref() {
            // refresh log4rs config in one process
            handle.set_config(config);
        } else {
            let handle = log4rs::init_config(config)?;
            *handle_guard = Some(handle);
        }
        Ok(())
    }

    async fn flush_monitors_generic<T1, T2>(
        interval_secs: u64,
        cancel_token: CancellationToken,
        t1_monitors: &[Arc<T1>],
        t2_monitors: &[Arc<T2>],
    ) where
        T1: FlushableMonitor + Send + Sync + 'static,
        T2: FlushableMonitor + Send + Sync + 'static,
    {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        interval.tick().await;

        loop {
            if cancel_token.is_cancelled() {
                Self::do_flush_monitors(t1_monitors, t2_monitors).await;
                break;
            }

            select! {
                _ = interval.tick() => {
                    Self::do_flush_monitors(t1_monitors, t2_monitors).await;
                }
                _ = cancel_token.cancelled() => {
                    log_info!("task shutdown detected, do final flush");
                    Self::do_flush_monitors(t1_monitors, t2_monitors).await;
                    break;
                }
            }
        }
    }

    /// Report every side that failed, in the given order, without letting a downstream
    /// cancellation error mask the real root cause.
    fn report_task_results(
        results: Vec<(&str, Result<anyhow::Result<()>, tokio::task::JoinError>)>,
    ) -> anyhow::Result<()> {
        let mut cancelled: Vec<String> = Vec::new();
        let mut failures: Vec<String> = Vec::new();
        for (name, result) in results {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    let msg = format!("{} failed: {:#}", name, e);
                    if e.chain()
                        .any(|cause| matches!(cause.downcast_ref::<Error>(), Some(Error::Cancelled(_))))
                    {
                        cancelled.push(msg);
                    } else {
                        failures.push(msg);
                    }
                }
                Err(e) => failures.push(format!("{} join error: {}", name, e)),
            }
        }

        if failures.is_empty() && cancelled.is_empty() {
            return Ok(());
        }
        if failures.is_empty() {
            bail!("{}", cancelled.join("; "));
        }
        for msg in cancelled {
            log_warn!("{} (follow-up of the failure above)", msg);
        }
        bail!("{}", failures.join("; "));
    }

    async fn do_flush_monitors<T1, T2>(t1_monitors: &[Arc<T1>], t2_monitors: &[Arc<T2>])
    where
        T1: FlushableMonitor + Send + Sync + 'static,
        T2: FlushableMonitor + Send + Sync + 'static,
    {
        let t1_futures = t1_monitors
            .iter()
            .map(|monitor| {
                let monitor = monitor.clone();
                async move { monitor.flush().await }
            })
            .collect::<Vec<_>>();

        let t2_futures = t2_monitors
            .iter()
            .map(|monitor| {
                let monitor = monitor.clone();
                async move { monitor.flush().await }
            })
            .collect::<Vec<_>>();

        tokio::join!(
            futures::future::join_all(t1_futures),
            futures::future::join_all(t2_futures)
        );
    }

    async fn pre_single_task(
        &self,
        extractor_client: ConnClient,
        sinker_client: ConnClient,
        sinker_data_marker: Option<DataMarker>,
    ) -> anyhow::Result<()> {
        // create heartbeat table
        let heartbeat_schema_tb = match &self.config.extractor {
            ExtractorConfig::MysqlCdc { heartbeat_tb, .. }
            | ExtractorConfig::PgCdc { heartbeat_tb, .. } => ConfigTokenParser::parse(
                heartbeat_tb,
                &['.'],
                &TokenEscapePair::from_char_pairs(SqlUtil::get_escape_pairs(
                    &self.config.extractor_basic.db_type,
                )),
            ),
            _ => vec![],
        };

        if heartbeat_schema_tb.len() == 2 {
            match &self.config.extractor {
                ExtractorConfig::MysqlCdc { .. } => {
                    let db_sql =
                        format!("CREATE DATABASE IF NOT EXISTS `{}`", heartbeat_schema_tb[0]);
                    let tb_sql = format!(
                        "CREATE TABLE IF NOT EXISTS `{}`.`{}`(
                        server_id INT UNSIGNED,
                        update_timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                        received_binlog_filename VARCHAR(255),
                        received_next_event_position INT UNSIGNED,
                        received_timestamp VARCHAR(255),
                        flushed_binlog_filename VARCHAR(255),
                        flushed_next_event_position INT UNSIGNED,
                        flushed_timestamp VARCHAR(255),
                        PRIMARY KEY(server_id)
                    )",
                        heartbeat_schema_tb[0], heartbeat_schema_tb[1]
                    );

                    TaskUtil::check_and_create_tb(
                        &extractor_client.clone(),
                        &heartbeat_schema_tb[0],
                        &heartbeat_schema_tb[1],
                        &db_sql,
                        &tb_sql,
                        &DbType::Mysql,
                    )
                    .await?
                }

                ExtractorConfig::PgCdc { .. } => {
                    let schema_sql = format!(
                        r#"CREATE SCHEMA IF NOT EXISTS "{}""#,
                        heartbeat_schema_tb[0]
                    );
                    let tb_sql = format!(
                        r#"CREATE TABLE IF NOT EXISTS "{}"."{}"(
                        slot_name character varying(64) not null,
                        update_timestamp timestamp without time zone default (now() at time zone 'utc'),
                        received_lsn character varying(64),
                        received_timestamp character varying(64),
                        flushed_lsn character varying(64),
                        flushed_timestamp character varying(64),
                        primary key(slot_name)
                    )"#,
                        heartbeat_schema_tb[0], heartbeat_schema_tb[1]
                    );

                    TaskUtil::check_and_create_tb(
                        &extractor_client.clone(),
                        &heartbeat_schema_tb[0],
                        &heartbeat_schema_tb[1],
                        &schema_sql,
                        &tb_sql,
                        &self.config.extractor_basic.db_type,
                    )
                    .await?
                }

                _ => {}
            }
        }

        // create data marker table
        if let Some(data_marker) = sinker_data_marker {
            match &self.config.sinker {
                SinkerConfig::Mysql { .. } => {
                    let db_sql = format!(
                        "CREATE DATABASE IF NOT EXISTS `{}`",
                        data_marker.marker_schema
                    );
                    let tb_sql = format!(
                        "CREATE TABLE IF NOT EXISTS `{}`.`{}` (
                            data_origin_node varchar(255) NOT NULL,
                            src_node varchar(255) NOT NULL,
                            dst_node varchar(255) NOT NULL,
                            n bigint DEFAULT NULL,
                            PRIMARY KEY (data_origin_node, src_node, dst_node)
                        )",
                        data_marker.marker_schema, data_marker.marker_tb
                    );

                    TaskUtil::check_and_create_tb(
                        &sinker_client.clone(),
                        &data_marker.marker_schema,
                        &data_marker.marker_tb,
                        &db_sql,
                        &tb_sql,
                        &DbType::Mysql,
                    )
                    .await?
                }

                SinkerConfig::Pg { .. } => {
                    let schema_sql = format!(
                        r#"CREATE SCHEMA IF NOT EXISTS "{}""#,
                        data_marker.marker_schema
                    );
                    let tb_sql = format!(
                        r#"CREATE TABLE IF NOT EXISTS "{}"."{}" (
                            data_origin_node varchar(255) NOT NULL,
                            src_node varchar(255) NOT NULL,
                            dst_node varchar(255) NOT NULL,
                            n bigint DEFAULT NULL,
                            PRIMARY KEY (data_origin_node, src_node, dst_node)
                        )"#,
                        data_marker.marker_schema, data_marker.marker_tb
                    );

                    TaskUtil::check_and_create_tb(
                        &sinker_client.clone(),
                        &data_marker.marker_schema,
                        &data_marker.marker_tb,
                        &schema_sql,
                        &tb_sql,
                        &self.config.sinker_basic.db_type,
                    )
                    .await?
                }

                _ => {}
            }
        }
        Ok(())
    }

    async fn build_pending_tasks(
        &self,
        original_task_context: TaskContext,
        is_multi_task: bool,
    ) -> anyhow::Result<VecDeque<TaskContext>> {
        let db_type = &self.config.extractor_basic.db_type;
        let filter = RdbFilter::from_config(&self.config.filter, db_type)?;

        let mut pending_tasks = VecDeque::new();

        let schemas =
            TaskUtil::list_schemas(&original_task_context.extractor_client.clone(), db_type)
                .await?
                .iter()
                .filter(|schema| !filter.filter_schema(schema))
                .map(|s| s.to_owned())
                .collect::<Vec<_>>();
        if schemas.is_empty() {
            log_warn!("no schemas to extract");
            return Ok(pending_tasks);
        }

        if is_multi_task {
            if let Some(task_type) = &self.task_type {
                log_info!("begin to estimate record count");
                let record_count = TaskUtil::estimate_record_count(
                    task_type,
                    &original_task_context.extractor_client.clone(),
                    db_type,
                    &schemas,
                    &filter,
                )
                .await?;
                log_info!("estimate record count: {}", record_count);

                self.task_monitor
                    .add_no_window_metrics(TaskMetricsType::ExtractorPlanRecords, record_count);
            }
        }

        let router = original_task_context.router.clone();
        let extractor_client = original_task_context.extractor_client.clone();
        let sinker_client = original_task_context.sinker_client.clone();

        let is_db_extractor_config = matches!(
            &self.config.extractor,
            ExtractorConfig::MysqlStruct { .. }
                | ExtractorConfig::PgStruct { .. }
                | ExtractorConfig::OracleStruct { .. }
        );
        if is_db_extractor_config {
            let db_extractor_config = match &self.config.extractor {
                ExtractorConfig::MysqlStruct {
                    url,
                    connection_auth,
                    db,
                    db_batch_size,
                    ..
                } => ExtractorConfig::MysqlStruct {
                    url: url.clone(),
                    connection_auth: connection_auth.clone(),
                    db: db.clone(),
                    dbs: schemas,
                    db_batch_size: *db_batch_size,
                },
                ExtractorConfig::PgStruct {
                    url,
                    connection_auth,
                    schema,
                    db_batch_size,
                    ..
                } => ExtractorConfig::PgStruct {
                    url: url.clone(),
                    connection_auth: connection_auth.clone(),
                    schema: schema.clone(),
                    schemas,
                    do_global_structs: true,
                    db_batch_size: *db_batch_size,
                },
                ExtractorConfig::OracleStruct {
                    url,
                    connection_auth,
                    schema,
                    db_batch_size,
                    ..
                } => ExtractorConfig::OracleStruct {
                    url: url.clone(),
                    connection_auth: connection_auth.clone(),
                    schema: schema.clone(),
                    schemas,
                    db_batch_size: *db_batch_size,
                },
                _ => {
                    bail! {Error::ConfigError("unsupported extractor config type".into())}
                }
            };
            pending_tasks.push_back(TaskContext {
                extractor_config: db_extractor_config,
                router,
                id: "".to_string(),
                extractor_client,
                sinker_client,
                recorder: original_task_context.recorder.clone(),
                recovery: original_task_context.recovery.clone(),
                check_summary: original_task_context.check_summary.clone(),
                partition_cols: original_task_context.partition_cols.clone(),
                enqueue_limiter: original_task_context.enqueue_limiter.clone(),
                dequeue_limiter: original_task_context.dequeue_limiter.clone(),
                cancel_token: original_task_context.cancel_token.clone(),
            });
        } else {
            for schema in schemas.iter() {
                // find pending tables
                let tbs = TaskUtil::list_tbs(
                    &original_task_context.extractor_client.clone(),
                    schema,
                    db_type,
                )
                .await?;

                self.task_monitor
                    .add_no_window_metrics(TaskMetricsType::TotalProgressCount, tbs.len() as u64);
                let mut finished_tbs = 0;

                for tb in tbs.iter() {
                    if let Some(recovery_handler) = original_task_context.recovery.as_ref() {
                        if recovery_handler.check_snapshot_finished(schema, tb).await {
                            log_info!("schema: {}, tb: {}, already finished", schema, tb);
                            finished_tbs += 1;
                            continue;
                        }
                    }

                    if filter.filter_event(schema, tb, &RowType::Insert) {
                        log_info!("schema: {}, tb: {}, insert events filtered", schema, tb);
                        continue;
                    }
                    let tb_extractor_config = match &self.config.extractor {
                        ExtractorConfig::MysqlSnapshot {
                            url,
                            connection_auth,
                            sample_interval,
                            parallel_size,
                            batch_size,
                            ..
                        } => ExtractorConfig::MysqlSnapshot {
                            url: url.clone(),
                            connection_auth: connection_auth.clone(),
                            db: schema.clone(),
                            tb: tb.clone(),
                            sample_interval: *sample_interval,
                            parallel_size: *parallel_size,
                            batch_size: *batch_size,
                            partition_cols: String::new(),
                        },

                        ExtractorConfig::PgSnapshot {
                            url,
                            connection_auth,
                            sample_interval,
                            parallel_size,
                            batch_size,
                            ..
                        } => ExtractorConfig::PgSnapshot {
                            url: url.clone(),
                            connection_auth: connection_auth.clone(),
                            schema: schema.clone(),
                            tb: tb.clone(),
                            sample_interval: *sample_interval,
                            parallel_size: *parallel_size,
                            batch_size: *batch_size,
                            partition_cols: String::new(),
                        },

                        ExtractorConfig::OracleSnapshot {
                            url,
                            connection_auth,
                            sample_interval,
                            parallel_size,
                            batch_size,
                            ..
                        } => ExtractorConfig::OracleSnapshot {
                            url: url.clone(),
                            connection_auth: connection_auth.clone(),
                            schema: schema.clone(),
                            tb: tb.clone(),
                            sample_interval: *sample_interval,
                            parallel_size: *parallel_size,
                            batch_size: *batch_size,
                            partition_cols: String::new(),
                        },

                        ExtractorConfig::MongoSnapshot {
                            url,
                            connection_auth,
                            app_name,
                            ..
                        } => ExtractorConfig::MongoSnapshot {
                            url: url.clone(),
                            connection_auth: connection_auth.clone(),
                            app_name: app_name.clone(),
                            db: schema.clone(),
                            tb: tb.clone(),
                        },

                        ExtractorConfig::FoxlakeS3 {
                            url,
                            s3_config,
                            batch_size,
                            ..
                        } => ExtractorConfig::FoxlakeS3 {
                            url: url.clone(),
                            schema: schema.clone(),
                            tb: tb.clone(),
                            s3_config: s3_config.clone(),
                            batch_size: *batch_size,
                        },

                        _ => {
                            bail! {Error::ConfigError("unsupported extractor config for `runtime.tb_parallel_size`".into())};
                        }
                    };
                    pending_tasks.push_back(TaskContext {
                        extractor_config: tb_extractor_config,
                        router: router.clone(),
                        id: format!("{}.{}", schema, tb),
                        extractor_client: extractor_client.clone(),
                        sinker_client: sinker_client.clone(),
                        recorder: original_task_context.recorder.clone(),
                        recovery: original_task_context.recovery.clone(),
                        check_summary: original_task_context.check_summary.clone(),
                        partition_cols: original_task_context.partition_cols.clone(),
                        enqueue_limiter: original_task_context.enqueue_limiter.clone(),
                        dequeue_limiter: original_task_context.dequeue_limiter.clone(),
                        cancel_token: original_task_context.cancel_token.clone(),
                    });
                }

                self.task_monitor.add_no_window_metrics(
                    TaskMetricsType::FinishedProgressCount,
                    finished_tbs as u64,
                );
            }
        }
        Ok(pending_tasks)
    }

    fn get_task_parallel_size(&self) -> usize {
        match &self.config.extractor {
            ExtractorConfig::MysqlSnapshot { .. }
            | ExtractorConfig::PgSnapshot { .. }
            | ExtractorConfig::OracleSnapshot { .. }
            | ExtractorConfig::FoxlakeS3 { .. }
            | ExtractorConfig::MongoSnapshot { .. } => self.config.runtime.tb_parallel_size,
            _ => 1,
        }
    }
}
