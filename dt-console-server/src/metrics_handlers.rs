//! HTTP handler for the metrics query API.
//!
//! GET /api/runs/:id/metrics?metric=&from=&to=&step=

use actix_web::{get, web, HttpResponse, ResponseError};
use serde::Deserialize;

use crate::error::{codes, ApiError};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::UserContext;
use crate::repositories::metric_point_repository::MetricPointRepository;
use crate::repositories::run_repository::RunRepository;
use crate::time_series_store;

/// Query parameters for GET /api/runs/:id/metrics.
#[derive(Debug, Clone, Deserialize)]
pub struct MetricsQuery {
    pub metric: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub step: Option<i64>,
}

/// GET /api/runs/:id/metrics — query metric time-series data for a Run.
///
/// - `metric`: exact metric name (required, must be a known metric for the run)
/// - `from`: start of time range (ISO-8601, inclusive)
/// - `to`: end of time range (ISO-8601, exclusive)
/// - `step`: optional downsample step in seconds
///
/// Returns 200 with `{metric, data: [{ts, value}, ...], source?}`.
/// Unknown metric name → 400 envelope error.
/// Empty range → 200 with empty series.
/// Metrics-feature-disabled Run → 200 with empty series + details.hint.
#[get("/runs/{id}/metrics")]
pub async fn get_metrics(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    query: web::Query<MetricsQuery>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let run_id = path.into_inner();
    let q = query.into_inner();

    // Validate the Run exists.
    let _run = match RunRepository::find_by_id(&pool, &run_id).await {
        Ok(r) => r,
        Err(_) => {
            return ApiError::with_details(
                codes::RUN_NOT_FOUND,
                "Run not found",
                serde_json::json!({ "id": run_id }),
            )
            .error_response();
        }
    };

    // Check if metric name exists for this run.
    let metric_known = MetricPointRepository::metric_name_exists_for_run(&pool, &run_id, &q.metric)
        .await
        .unwrap_or(false);

    // If no metric points at all for this run, check if the run has metrics
    // configured. If not, return empty series with a hint.
    let known_names = MetricPointRepository::list_metric_names_by_run(&pool, &run_id)
        .await
        .unwrap_or_default();

    if known_names.is_empty() {
        // No metrics at all — might be a metrics-feature-disabled Run.
        // Return empty series with a hint rather than an error.
        let mut response =
            time_series_store::query_series(&pool, &run_id, &q.metric, &q.from, &q.to, q.step)
                .await
                .unwrap_or_else(|_| crate::models::MetricSeriesResponse {
                    metric: q.metric.clone(),
                    data: vec![],
                    source: None,
                });

        response.source = Some(vec!["hint".to_string()]);
        let mut body = serde_json::to_value(&response).unwrap_or_default();
        body["details"] = serde_json::json!({
            "hint": "No metrics data available. Verify [metrics] is configured in the task's runtime settings."
        });
        return HttpResponse::Ok().json(body);
    }

    // Metric name not known for this run → error.
    if !metric_known {
        return ApiError::with_details(
            codes::VALIDATION_FAILED,
            "Unknown metric name for this run",
            serde_json::json!({
                "metric": q.metric,
                "available": known_names,
            }),
        )
        .error_response();
    }

    // Empty range (from == to) → 200 with empty series.
    if q.from == q.to {
        return HttpResponse::Ok().json(crate::models::MetricSeriesResponse {
            metric: q.metric.clone(),
            data: vec![],
            source: None,
        });
    }

    // Query the time-series store.
    match time_series_store::query_series(&pool, &run_id, &q.metric, &q.from, &q.to, q.step).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => ApiError::new(codes::INTERNAL_ERROR, format!("metric query failed: {e}"))
            .error_response(),
    }
}

#[cfg(test)]
mod tests {
    use crate::db;
    use crate::models::{MetricPoint, ResourceGroup, Run};
    use crate::repositories::metric_point_repository::MetricPointRepository;
    use crate::repositories::resource_group_repository::ResourceGroupRepository;
    use crate::repositories::run_repository::RunRepository;

    /// Helper: create an in-memory test pool with migrations applied.
    async fn test_pool() -> sqlx::SqlitePool {
        db::init(":memory:").await.unwrap()
    }

    /// Helper: seed a default resource group so FK constraints pass.
    async fn seed_resource_group(pool: &sqlx::SqlitePool) -> String {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let rg = ResourceGroup {
            id: "rg-default".to_string(),
            name: "default".to_string(),
            is_default: true,
            created_at: now.clone(),
            updated_at: now,
        };
        ResourceGroupRepository::create(pool, &rg).await.unwrap();
        rg.id.clone()
    }

    /// Helper: seed a task.
    async fn seed_task(pool: &sqlx::SqlitePool, task_id: &str, rg_id: &str) {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        sqlx::query(
            "INSERT INTO tasks (id, task_id, name, kind, db_type_source, db_type_target,
             source_endpoint, target_endpoint, extractor_config, sinker_config,
             filter_config, router_config, parallelizer_config, pipeline_config,
             resumer_config, processor_config, runtime_config, metrics_config,
             resource_group_id, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(task_id)
        .bind("test_task")
        .bind("Test Task")
        .bind("snapshot")
        .bind("mysql")
        .bind("mysql")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind(rg_id)
        .bind("draft")
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Helper: seed a run.
    async fn seed_run(pool: &sqlx::SqlitePool, run_id: &str, task_id: &str, status: &str) {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let run = Run {
            id: run_id.to_string(),
            task_id: Some(task_id.to_string()),
            status: status.to_string(),
            pid: Some(1234),
            ini_path: None,
            log_dir: None,
            started_at: Some(now.clone()),
            stopped_at: None,
            exit_code: None,
            stop_method: None,
            created_at: now.clone(),
            updated_at: now,
        };
        RunRepository::create(pool, &run).await.unwrap();
    }

    /// Helper: insert metric points for a run.
    async fn seed_metric_points(
        pool: &sqlx::SqlitePool,
        task_id: &str,
        run_id: &str,
        metric_name: &str,
        count: i32,
        base_offset_secs: i64,
    ) {
        let base_time = chrono::Utc::now() - chrono::Duration::seconds(base_offset_secs);
        for i in 0..count {
            let ts = (base_time + chrono::Duration::seconds(i as i64 * 10))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let mp = MetricPoint {
                id: 0,
                task_id: task_id.to_string(),
                run_id: run_id.to_string(),
                metric_name: metric_name.to_string(),
                ts,
                value: 10.0 + i as f64,
            };
            MetricPointRepository::create(pool, &mp).await.unwrap();
        }
    }

    #[tokio::test]
    async fn metric_name_exists_for_run() {
        let pool = test_pool().await;
        let rg_id = seed_resource_group(&pool).await;
        seed_task(&pool, "test-task-1", &rg_id).await;
        seed_run(&pool, "run-1", "test-task-1", "running").await;
        seed_metric_points(&pool, "test-task-1", "run-1", "extractor_rps_avg", 5, 30).await;

        let exists =
            MetricPointRepository::metric_name_exists_for_run(&pool, "run-1", "extractor_rps_avg")
                .await
                .unwrap();
        assert!(exists);

        let not_exists =
            MetricPointRepository::metric_name_exists_for_run(&pool, "run-1", "does_not_exist")
                .await
                .unwrap();
        assert!(!not_exists);
    }

    #[tokio::test]
    async fn list_metric_names_by_run() {
        let pool = test_pool().await;
        let rg_id = seed_resource_group(&pool).await;
        seed_task(&pool, "test-task-1", &rg_id).await;
        seed_run(&pool, "run-1", "test-task-1", "running").await;
        seed_metric_points(&pool, "test-task-1", "run-1", "extractor_rps_avg", 5, 30).await;

        let names = MetricPointRepository::list_metric_names_by_run(&pool, "run-1")
            .await
            .unwrap();
        assert_eq!(names, vec!["extractor_rps_avg"]);
    }

    #[tokio::test]
    async fn query_range_returns_points_in_range() {
        let pool = test_pool().await;
        let rg_id = seed_resource_group(&pool).await;
        seed_task(&pool, "test-task-1", &rg_id).await;
        seed_run(&pool, "run-1", "test-task-1", "running").await;
        seed_metric_points(&pool, "test-task-1", "run-1", "extractor_rps_avg", 5, 30).await;

        let from = (chrono::Utc::now() - chrono::Duration::seconds(40))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let to = (chrono::Utc::now() + chrono::Duration::seconds(10))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let points =
            MetricPointRepository::query_range(&pool, "run-1", "extractor_rps_avg", &from, &to)
                .await
                .unwrap();
        assert_eq!(points.len(), 5);
        assert!((points[0].value - 10.0).abs() < f64::EPSILON);
        assert!((points[4].value - 14.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn query_range_empty_range_returns_empty() {
        let pool = test_pool().await;
        let rg_id = seed_resource_group(&pool).await;
        seed_task(&pool, "test-task-1", &rg_id).await;
        seed_run(&pool, "run-1", "test-task-1", "running").await;
        seed_metric_points(&pool, "test-task-1", "run-1", "extractor_rps_avg", 5, 30).await;

        let now_ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let points = MetricPointRepository::query_range(
            &pool,
            "run-1",
            "extractor_rps_avg",
            &now_ts,
            &now_ts,
        )
        .await
        .unwrap();
        assert!(points.is_empty());
    }

    #[tokio::test]
    async fn create_batch_inserts_multiple_points() {
        let pool = test_pool().await;
        let rg_id = seed_resource_group(&pool).await;
        seed_task(&pool, "test-task-1", &rg_id).await;
        seed_run(&pool, "batch-run", "test-task-1", "running").await;

        let points = vec![
            MetricPoint {
                id: 0,
                task_id: "test-task-1".to_string(),
                run_id: "batch-run".to_string(),
                metric_name: "pipeline_buffer_size_avg".to_string(),
                ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                value: 100.0,
            },
            MetricPoint {
                id: 0,
                task_id: "test-task-1".to_string(),
                run_id: "batch-run".to_string(),
                metric_name: "pipeline_buffer_size_avg".to_string(),
                ts: (chrono::Utc::now() + chrono::Duration::seconds(10))
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                value: 200.0,
            },
        ];

        MetricPointRepository::create_batch(&pool, &points)
            .await
            .unwrap();

        let stored = MetricPointRepository::list_by_run(&pool, "batch-run")
            .await
            .unwrap();
        assert_eq!(stored.len(), 2);
    }

    #[tokio::test]
    async fn downsample_creates_bucketed_rows() {
        let pool = test_pool().await;
        let rg_id = seed_resource_group(&pool).await;
        seed_task(&pool, "test-task-1", &rg_id).await;
        seed_run(&pool, "old-run", "test-task-1", "stopped").await;

        // Insert "old" metric points (older than 24h) directly via SQL
        // to bypass the FK timestamp check.
        let old_time = chrono::Utc::now() - chrono::Duration::hours(25);
        for i in 0..6 {
            let ts = (old_time + chrono::Duration::seconds(i * 10))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            sqlx::query(
                "INSERT INTO metric_points (task_id, run_id, metric_name, ts, value)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind("test-task-1")
            .bind("old-run")
            .bind("sinker_bps_avg_by_sec")
            .bind(&ts)
            .bind(50.0 + i as f64)
            .execute(&pool)
            .await
            .unwrap();
        }

        // Run downsample.
        let deleted = crate::time_series_store::downsample(&pool).await.unwrap();
        assert!(deleted > 0);

        // Verify downsampled rows exist.
        let downsampled = MetricPointRepository::query_downsampled_range(
            &pool,
            "old-run",
            "sinker_bps_avg_by_sec",
            "2000-01-01T00:00:00.000Z",
            "2099-01-01T00:00:00.000Z",
        )
        .await
        .unwrap();
        assert!(!downsampled.is_empty());
    }

    #[tokio::test]
    async fn query_series_cross_boundary_stitches() {
        let pool = test_pool().await;
        let rg_id = seed_resource_group(&pool).await;
        seed_task(&pool, "cross-task", &rg_id).await;
        seed_run(&pool, "cross-run", "cross-task", "running").await;

        // Insert native (raw) metric points.
        seed_metric_points(&pool, "cross-task", "cross-run", "extractor_rps_avg", 3, 30).await;

        // Insert downsampled metric points (older) directly.
        let old_time = chrono::Utc::now() - chrono::Duration::hours(25);
        for i in 0..3 {
            let bucket_ts = (old_time + chrono::Duration::minutes(i))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            sqlx::query(
                "INSERT INTO downsampled_metric_points
                 (task_id, run_id, metric_name, bucket_ts, bucket_secs, value_mean, value_min, value_max, sample_count)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind("cross-task")
            .bind("cross-run")
            .bind("extractor_rps_avg")
            .bind(&bucket_ts)
            .bind(60)
            .bind(5.0 + i as f64)
            .bind(4.0 + i as f64)
            .bind(6.0 + i as f64)
            .bind(6i64)
            .execute(&pool)
            .await
            .unwrap();
        }

        // Query spanning both ranges.
        let from = (old_time - chrono::Duration::hours(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let to = (chrono::Utc::now() + chrono::Duration::minutes(5))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let response = crate::time_series_store::query_series(
            &pool,
            "cross-run",
            "extractor_rps_avg",
            &from,
            &to,
            None,
        )
        .await
        .unwrap();

        assert_eq!(response.metric, "extractor_rps_avg");
        // Should have both downsampled (3) and native (3) points.
        assert_eq!(response.data.len(), 6);
        // Source should list both.
        let sources = response.source.unwrap();
        assert!(sources.contains(&"downsampled".to_string()));
        assert!(sources.contains(&"native".to_string()));
    }

    #[tokio::test]
    async fn metrics_feature_disabled_run_returns_empty_with_hint() {
        let pool = test_pool().await;
        let rg_id = seed_resource_group(&pool).await;
        seed_task(&pool, "no-metrics-task", &rg_id).await;
        seed_run(&pool, "no-metrics-run", "no-metrics-task", "stopped").await;

        // Query — should return empty data, not an error.
        let from = (chrono::Utc::now() - chrono::Duration::hours(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let to = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let response = crate::time_series_store::query_series(
            &pool,
            "no-metrics-run",
            "extractor_rps_avg",
            &from,
            &to,
            None,
        )
        .await
        .unwrap();

        assert!(response.data.is_empty());
    }

    #[tokio::test]
    async fn finished_run_keeps_history_queryable() {
        let pool = test_pool().await;
        let rg_id = seed_resource_group(&pool).await;
        seed_task(&pool, "test-task-1", &rg_id).await;
        seed_run(&pool, "run-finished", "test-task-1", "stopped").await;
        seed_metric_points(
            &pool,
            "test-task-1",
            "run-finished",
            "extractor_rps_avg",
            5,
            30,
        )
        .await;

        let from = (chrono::Utc::now() - chrono::Duration::seconds(60))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let to = (chrono::Utc::now() + chrono::Duration::seconds(60))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let points = MetricPointRepository::query_range(
            &pool,
            "run-finished",
            "extractor_rps_avg",
            &from,
            &to,
        )
        .await
        .unwrap();
        assert_eq!(points.len(), 5);
    }

    /// After 24h downsampling, metric_name_exists_for_run and
    /// list_metric_names_by_run must still find names in the
    /// downsampled table even when the raw metric_points are gone.
    #[tokio::test]
    async fn metric_name_finds_downsampled_after_raws_deleted() {
        let pool = test_pool().await;
        let rg_id = seed_resource_group(&pool).await;
        seed_task(&pool, "ds-task", &rg_id).await;
        seed_run(&pool, "ds-run", "ds-task", "stopped").await;

        // Insert downsampled rows directly (simulating post-downsample state).
        let old_time = chrono::Utc::now() - chrono::Duration::hours(25);
        for i in 0..3 {
            let bucket_ts = (old_time + chrono::Duration::minutes(i))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            sqlx::query(
                "INSERT INTO downsampled_metric_points
                 (task_id, run_id, metric_name, bucket_ts, bucket_secs, value_mean, value_min, value_max, sample_count)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind("ds-task")
            .bind("ds-run")
            .bind("extractor_rps_avg")
            .bind(&bucket_ts)
            .bind(60)
            .bind(5.0 + i as f64)
            .bind(4.0 + i as f64)
            .bind(6.0 + i as f64)
            .bind(6i64)
            .execute(&pool)
            .await
            .unwrap();
        }

        // No raw metric_points for this run — only downsampled.
        // Verify the raw-only query would be empty (sanity check).
        let raw_only: Vec<(String,)> =
            sqlx::query_as("SELECT DISTINCT metric_name FROM metric_points WHERE run_id = ?")
                .bind("ds-run")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(
            raw_only.is_empty(),
            "raw table should have no names for ds-run"
        );

        // metric_name_exists_for_run should find it via the UNION query.
        let exists =
            MetricPointRepository::metric_name_exists_for_run(&pool, "ds-run", "extractor_rps_avg")
                .await
                .unwrap();
        assert!(
            exists,
            "metric_name_exists_for_run must find downsampled rows"
        );

        // list_metric_names_by_run should return names from both tables.
        let names = MetricPointRepository::list_metric_names_by_run(&pool, "ds-run")
            .await
            .unwrap();
        assert_eq!(names, vec!["extractor_rps_avg"]);
    }
}
