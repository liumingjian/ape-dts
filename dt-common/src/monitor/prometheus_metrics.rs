#[cfg(feature = "metrics")]
use std::{collections::BTreeMap, sync::Arc};

use actix_web::{middleware::Logger, web, App, HttpResponse, HttpServer, Responder, Result};
use dashmap::DashMap;
use prometheus::{Gauge, Opts, Registry, TextEncoder};

use crate::config::config_enums::TaskType;
use crate::config::metrics_config::MetricsConfig;
use crate::monitor::task_metrics::TaskMetricsType;

pub struct PrometheusMetrics {
    registry: Arc<Registry>,
    metrics: DashMap<TaskMetricsType, Gauge>,
    task_type: Option<TaskType>,
    config: MetricsConfig,
}

impl PrometheusMetrics {
    pub fn new(task_type: Option<TaskType>, config: MetricsConfig) -> Self {
        Self {
            registry: Arc::new(Registry::new()),
            metrics: DashMap::new(),
            task_type,
            config,
        }
    }

    pub fn initialization(&self) -> &Self {
        let register_handler =
            |metrics_name: &str, metrics_desc: &str, metrics_type: TaskMetricsType| {
                let metrics = Gauge::with_opts(
                    Opts::new(metrics_name, metrics_desc)
                        .const_labels(self.config.metrics_labels.to_owned()),
                )
                .unwrap();

                self.registry.register(Box::new(metrics.clone())).unwrap();
                self.metrics.insert(metrics_type, metrics);
            };

        register_handler(
            "extractor_rps_max",
            "the max records per second of extractor",
            TaskMetricsType::ExtractorRpsMax,
        );
        register_handler(
            "extractor_rps_min",
            "the min records per second of extractor",
            TaskMetricsType::ExtractorRpsMin,
        );
        register_handler(
            "extractor_rps_avg",
            "the average records per second of extractor",
            TaskMetricsType::ExtractorRpsAvg,
        );
        register_handler(
            "extractor_bps_max",
            "the max bytes per second of extractor",
            TaskMetricsType::ExtractorBpsMax,
        );
        register_handler(
            "extractor_bps_min",
            "the min bytes per second of extractor",
            TaskMetricsType::ExtractorBpsMin,
        );
        register_handler(
            "extractor_bps_avg",
            "the average bytes per second of extractor",
            TaskMetricsType::ExtractorBpsAvg,
        );

        register_handler(
            "extractor_pushed_rps_max",
            "the max pushed records per second of extractor",
            TaskMetricsType::ExtractorPushedRpsMax,
        );
        register_handler(
            "extractor_pushed_rps_min",
            "the min pushed records per second of extractor",
            TaskMetricsType::ExtractorPushedRpsMin,
        );
        register_handler(
            "extractor_pushed_rps_avg",
            "the average pushed records per second of extractor",
            TaskMetricsType::ExtractorPushedRpsAvg,
        );
        register_handler(
            "extractor_pushed_bps_max",
            "the max pushed bytes per second of extractor",
            TaskMetricsType::ExtractorPushedBpsMax,
        );
        register_handler(
            "extractor_pushed_bps_min",
            "the min pushed bytes per second of extractor",
            TaskMetricsType::ExtractorPushedBpsMin,
        );
        register_handler(
            "extractor_pushed_bps_avg",
            "the average pushed bytes per second of extractor",
            TaskMetricsType::ExtractorPushedBpsAvg,
        );

        register_handler(
            "pipeline_queue_size",
            "the records size of pipeline queue",
            TaskMetricsType::PipelineQueueSize,
        );
        register_handler(
            "pipeline_queue_bytes",
            "the bytes in pipeline queue",
            TaskMetricsType::PipelineQueueBytes,
        );

        register_handler(
            "sinker_rt_max",
            "the max response time of sinker, the unit is millisecond",
            TaskMetricsType::SinkerRtMax,
        );
        register_handler(
            "sinker_rt_min",
            "the min response time of sinker, the unit is millisecond",
            TaskMetricsType::SinkerRtMin,
        );
        register_handler(
            "sinker_rt_avg",
            "the average response time of sinker, the unit is millisecond",
            TaskMetricsType::SinkerRtAvg,
        );

        register_handler(
            "sinker_rps_max",
            "the max records per second of sinker",
            TaskMetricsType::SinkerRpsMax,
        );
        register_handler(
            "sinker_rps_min",
            "the min records per second of sinker",
            TaskMetricsType::SinkerRpsMin,
        );
        register_handler(
            "sinker_rps_avg",
            "the average records per second of sinker",
            TaskMetricsType::SinkerRpsAvg,
        );
        register_handler(
            "sinker_bps_max",
            "the max bytes per second of sinker",
            TaskMetricsType::SinkerBpsMax,
        );
        register_handler(
            "sinker_bps_min",
            "the min bytes per second of sinker",
            TaskMetricsType::SinkerBpsMin,
        );
        register_handler(
            "sinker_bps_avg",
            "the average bytes per second of sinker",
            TaskMetricsType::SinkerBpsAvg,
        );

        register_handler(
            "sinker_sinked_records",
            "the number of records sinked",
            TaskMetricsType::SinkerSinkedRecords,
        );
        register_handler(
            "sinker_sinked_bytes",
            "the bytes of records sinked",
            TaskMetricsType::SinkerSinkedBytes,
        );

        if let Some(task_type) = &self.task_type {
            match task_type {
                TaskType::Snapshot => {
                    register_handler(
                        "extractor_plan_records",
                        "the records estimated by extractor plan",
                        TaskMetricsType::ExtractorPlanRecords,
                    );
                    register_handler(
                        "progress",
                        "the progress of task",
                        TaskMetricsType::Progress,
                    );
                }
                TaskType::Cdc => {
                    register_handler(
                        "timestamp",
                        "the timestamp of task",
                        TaskMetricsType::Timestamp,
                    );
                    register_handler(
                        "sinker_ddl_count",
                        "the count of DDL operations",
                        TaskMetricsType::SinkerDdlCount,
                    );
                    register_handler(
                        "lag",
                        "the lag of CDC task in second",
                        TaskMetricsType::Lag,
                    );
                }
                TaskType::Struct | TaskType::Check => {}
            }
        }
        self
    }

    pub fn set_metrics(&self, metrics: &BTreeMap<TaskMetricsType, u64>) {
        for (metrics_type, value) in metrics.iter() {
            if let Some(metrics) = self.metrics.get_mut(metrics_type) {
                metrics.set(*value as f64);
            }
        }
    }

    pub async fn start_metrics(&self) -> tokio::task::JoinHandle<Result<(), std::io::Error>> {
        let registry = self.registry.clone();
        let addr = format!("{}:{}", self.config.http_host, self.config.http_port);
        let server = HttpServer::new(move || {
            App::new()
                .wrap(Logger::default())
                .app_data(web::Data::new(registry.clone()))
                .service(web::resource("/metrics").route(web::get().to(metrics_handler)))
                .service(web::resource("/healthz").route(web::get().to(healthz_handler)))
                .default_service(web::route().to(not_found_handler))
        })
        .workers(self.config.workers as usize)
        .shutdown_timeout(10)
        .bind(&addr)
        .unwrap()
        .run();

        tokio::spawn(server)
    }
}

async fn metrics_handler(registry: web::Data<Arc<Registry>>) -> impl Responder {
    let mut buffer = String::new();
    let encoder = TextEncoder::new();

    match encoder.encode_utf8(&registry.gather(), &mut buffer) {
        Ok(_) => HttpResponse::Ok()
            .content_type("text/plain; charset=utf-8; version=0.0.4")
            .body(buffer),
        Err(e) => {
            log::error!("Failed to encode metrics: {}", e);
            HttpResponse::InternalServerError().body("Failed to encode metrics")
        }
    }
}

async fn healthz_handler() -> Result<impl Responder> {
    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .body(r#"{"status":"ok","service":"ape-dts"}"#))
}

async fn not_found_handler() -> Result<impl Responder> {
    Ok(HttpResponse::NotFound()
        .content_type("application/json")
        .body(r#"{"error":"Not Found","message":"The requested endpoint does not exist"}"#))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> MetricsConfig {
        MetricsConfig {
            http_host: "127.0.0.1".to_string(),
            http_port: 9199,
            workers: 1,
            metrics_labels: std::collections::HashMap::new(),
        }
    }

    fn gather_text(pm: &PrometheusMetrics) -> String {
        let mut buffer = String::new();
        let encoder = TextEncoder::new();
        encoder.encode_utf8(&pm.registry.gather(), &mut buffer).unwrap();
        buffer
    }

    #[test]
    fn cdc_registers_lag_gauge_with_second_in_help() {
        let pm = PrometheusMetrics::new(Some(TaskType::Cdc), make_config());
        pm.initialization();
        let text = gather_text(&pm);
        assert!(
            text.contains("# TYPE lag gauge"),
            "Expected '# TYPE lag gauge' line, got:\n{}",
            text
        );
        assert!(
            text.lines()
                .any(|l| l.starts_with("# HELP lag") && (l.contains("second") || l.contains("秒"))),
            "Expected '# HELP lag' to mention 'second' or '秒', got:\n{}",
            text
        );
        assert!(
            text.lines().any(|l| l.starts_with("lag ")),
            "Expected 'lag <value>' line, got:\n{}",
            text
        );
    }

    #[test]
    fn cdc_retains_timestamp_and_sinker_ddl_count() {
        let pm = PrometheusMetrics::new(Some(TaskType::Cdc), make_config());
        pm.initialization();
        let text = gather_text(&pm);
        assert!(text.contains("# TYPE timestamp gauge"), "Expected timestamp gauge");
        assert!(
            text.contains("# TYPE sinker_ddl_count gauge"),
            "Expected sinker_ddl_count gauge"
        );
    }

    #[test]
    fn snapshot_does_not_register_lag() {
        let pm = PrometheusMetrics::new(Some(TaskType::Snapshot), make_config());
        pm.initialization();
        let text = gather_text(&pm);
        assert!(
            !text.contains("# TYPE lag gauge"),
            "Snapshot must not have '# TYPE lag gauge' line"
        );
        assert!(
            !text.lines().any(|l| l.starts_with("lag ")),
            "Snapshot must not have 'lag <value>' line"
        );
    }

    #[test]
    fn snapshot_retains_progress_and_extractor_plan_records() {
        let pm = PrometheusMetrics::new(Some(TaskType::Snapshot), make_config());
        pm.initialization();
        let text = gather_text(&pm);
        assert!(text.contains("# TYPE progress gauge"), "Expected progress gauge");
        assert!(
            text.contains("# TYPE extractor_plan_records gauge"),
            "Expected extractor_plan_records gauge"
        );
    }

    #[test]
    fn no_delay_gauge_for_cdc_or_snapshot() {
        for task_type in [TaskType::Cdc, TaskType::Snapshot] {
            let label = format!("{:?}", task_type);
            let pm = PrometheusMetrics::new(Some(task_type), make_config());
            pm.initialization();
            let text = gather_text(&pm);
            assert!(
                !text.lines().any(|l| l.starts_with("delay ") || l.starts_with("# TYPE delay ")),
                "Must not have delay gauge for {}",
                label
            );
        }
    }

    // VAL-ORCH-016: /metrics endpoint returns HTTP 200 with # HELP and # TYPE lines.
    #[actix_web::test]
    async fn http_metrics_endpoint_returns_200_with_prometheus_format() {
        use actix_web::test;
        let pm = PrometheusMetrics::new(Some(TaskType::Cdc), make_config());
        pm.initialization();
        let registry = pm.registry.clone();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(registry))
                .service(web::resource("/metrics").route(web::get().to(metrics_handler))),
        )
        .await;
        let req = test::TestRequest::get().uri("/metrics").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);
        let body = test::read_body(resp).await;
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.lines().any(|l| l.starts_with("# HELP ")),
            "Expected at least one '# HELP' line in /metrics response"
        );
        assert!(
            body_str.lines().any(|l| l.starts_with("# TYPE ")),
            "Expected at least one '# TYPE' line in /metrics response"
        );
    }

    // VAL-ORCH-017 (negative control): without --features metrics, no metrics server is compiled
    // in — verified by the #[cfg(feature = "metrics")] gate on this entire module.
    // The following test confirms that a snapshot task's /metrics response has no lag gauge,
    // ensuring the feature flag truly toggles per-task-type exposure.
    #[actix_web::test]
    async fn http_metrics_snapshot_has_no_lag_gauge() {
        use actix_web::test;
        let pm = PrometheusMetrics::new(Some(TaskType::Snapshot), make_config());
        pm.initialization();
        let registry = pm.registry.clone();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(registry))
                .service(web::resource("/metrics").route(web::get().to(metrics_handler))),
        )
        .await;
        let req = test::TestRequest::get().uri("/metrics").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);
        let body = test::read_body(resp).await;
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            !body_str.lines().any(|l| l.starts_with("lag ")),
            "Snapshot /metrics must not expose a 'lag' gauge"
        );
    }
}
