use actix_session::storage::CookieSessionStore;
use actix_session::SessionMiddleware;
use actix_web::cookie::{Cookie, Key, SameSite};
use actix_web::http::StatusCode;
use actix_web::test;
use actix_web::web::{self, JsonConfig};
use actix_web::App;
use dt_console_server::auth;
use dt_console_server::error;
use dt_console_server::middleware::csrf::{Csrf, XSRF_COOKIE_NAME, XSRF_HEADER_NAME};
use dt_console_server::models::{LoginRequest, MetricPoint, ResourceGroup, Run};
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use dt_console_server::repositories::license_repository::LicenseRepository;
use dt_console_server::repositories::metric_point_repository::MetricPointRepository;
use dt_console_server::repositories::resource_group_repository::ResourceGroupRepository;
use dt_console_server::repositories::run_repository::RunRepository;
use dt_console_server::{log_sse_handlers, metrics_scraper, run_handlers};
use sqlx::SqlitePool;

const XSRF: &str = "test-xsrf-token";

async fn test_pool() -> SqlitePool {
    let path = std::env::temp_dir().join(format!("task-detail-{}.db", uuid::Uuid::new_v4()));
    let pool = dt_console_server::db::create_pool(path.to_str().unwrap())
        .await
        .unwrap();
    dt_console_server::db::run_migrations(&pool).await.unwrap();
    pool
}

fn build_test_app(
    pool: SqlitePool,
) -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    let key = Key::generate();
    let session = SessionMiddleware::builder(CookieSessionStore::default(), key)
        .cookie_name("session".to_string())
        .cookie_secure(false)
        .cookie_http_only(true)
        .cookie_same_site(SameSite::Lax)
        .cookie_path("/".to_string())
        .build();
    App::new()
        .wrap(session)
        .wrap(Csrf)
        .app_data(JsonConfig::default().error_handler(|err, _| {
            error::ApiError::new(error::codes::PARSE_ERROR, err.to_string()).into()
        }))
        .app_data(web::Data::new(pool))
        .app_data(web::Data::new(RateLimiter::new(RateLimitConfig::default())))
        .app_data(web::Data::new(3600_i64))
        .app_data(web::Data::new(run_handlers::new_active_runs()))
        .app_data(web::Data::new(metrics_scraper::ScraperState::new()))
        .app_data(web::Data::new(dt_console_server::port_pool::PortPool::new()))
        .app_data(web::Data::new(log_sse_handlers::LogSseState::default()))
        .app_data(web::Data::new(
            dt_console_server::alert_handlers::AlertSseState::new(),
        ))
        .app_data(web::Data::new(
            dt_console_server::alarm_dispatcher::DispatcherState::new(),
        ))
        .app_data(web::Data::new(
            dt_console_server::alert_engine::AlertEngineState::new(),
        ))
        .app_data(web::Data::new(
            dt_console_server::idempotency::IdempotencyCache::new(),
        ))
        .app_data(web::Data::new(
            dt_console_server::sse_session_tracker::SseSessionTracker::new(),
        ))
        .configure(dt_console_server::configure)
}

async fn setup(pool: &SqlitePool) {
    auth::seed_admin(pool).await.unwrap();
    let now = "2026-07-18T00:00:00.000Z".to_string();
    ResourceGroupRepository::create(
        pool,
        &ResourceGroup {
            id: "rg-default".into(),
            name: "default".into(),
            is_default: true,
            created_at: now.clone(),
            updated_at: now.clone(),
        },
    )
    .await
    .unwrap();
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update("pro:100:2030-12-31T23:59:59Z:test:ape-dts-console-license-secret-2025");
    LicenseRepository::create(
        pool,
        &dt_console_server::models::License {
            id: "license".into(),
            sku: "pro".into(),
            max_tasks: 100,
            expire_at: Some("2030-12-31T23:59:59Z".into()),
            activated_at: Some(now.clone()),
            activation_code_hash: Some(format!("{:x}", hasher.finalize())[..16].to_string()),
            granted_to: "test".into(),
            created_at: now.clone(),
            updated_at: now,
        },
    )
    .await
    .unwrap();
}

async fn seed_task(pool: &SqlitePool, id: &str, extract_type: &str) {
    sqlx::query(
        "INSERT INTO tasks (id, task_id, name, kind, db_type_source, db_type_target,
         source_endpoint, target_endpoint, extractor_config, sinker_config, filter_config,
         router_config, parallelizer_config, pipeline_config, resumer_config, processor_config,
         runtime_config, metrics_config, resource_group_id, status, created_at, updated_at)
         VALUES (?, ?, 'Task', 'snapshot', 'mysql', 'pg', '{}', '{}', ?, '{}',
         '{\"do_tbs\":[\"app.orders\"]}', '{}', '{}', '{}', '{}', '{}', '{}', '{}',
         'rg-default', 'running', '2026-07-18T00:00:00.000Z', '2026-07-18T00:00:00.000Z')",
    )
    .bind(id)
    .bind(format!("external-{id}"))
    .bind(format!(r#"{{"extract_type":"{extract_type}"}}"#))
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_run(pool: &SqlitePool, task_id: &str, status: &str, log_dir: Option<String>) {
    RunRepository::create(
        pool,
        &Run {
            id: format!("run-{task_id}"),
            task_id: Some(task_id.into()),
            status: status.into(),
            pid: None,
            ini_path: None,
            log_dir,
            started_at: Some("2026-07-18T00:01:00.000Z".into()),
            stopped_at: if matches!(status, "stopped" | "failed") {
                Some("2026-07-18T00:02:00.000Z".into())
            } else {
                None
            },
            exit_code: (status == "failed").then_some(1),
            stop_method: None,
            metrics_port: None,
            created_at: "2026-07-18T00:01:00.000Z".into(),
            updated_at: "2026-07-18T00:01:00.000Z".into(),
        },
    )
    .await
    .unwrap();
}

async fn metric_for_run(
    pool: &SqlitePool,
    task_id: &str,
    run_id: &str,
    name: &str,
    ts: &str,
    value: f64,
) {
    MetricPointRepository::create(
        pool,
        &MetricPoint {
            id: 0,
            task_id: task_id.into(),
            run_id: run_id.into(),
            metric_name: name.into(),
            ts: ts.into(),
            value,
        },
    )
    .await
    .unwrap();
}

async fn metric(pool: &SqlitePool, task: &str, name: &str, value: f64) {
    metric_for_run(
        pool,
        task,
        &format!("run-{task}"),
        name,
        "2026-07-18T00:01:30.000Z",
        value,
    )
    .await;
}

async fn login(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
        Error = actix_web::Error,
    >,
) -> Vec<Cookie<'static>> {
    let request = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "admin".into(),
            password: "admin123".into(),
        })
        .to_request();
    let response = test::call_service(app, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    response
        .headers()
        .get_all("set-cookie")
        .filter_map(|value| Cookie::parse_encoded(value.to_str().ok()?.to_string()).ok())
        .map(Cookie::into_owned)
        .collect()
}

fn get(uri: &str, cookies: &[Cookie<'static>]) -> actix_http::Request {
    let mut request = test::TestRequest::get().uri(uri);
    for cookie in cookies {
        request = request.cookie(cookie.clone());
    }
    request.to_request()
}

#[actix_web::test]
async fn list_tasks_includes_latest_run_and_nullable_progress() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "list-failed", "snapshot_and_cdc").await;
    seed_run(&pool, "list-failed", "failed", None).await;
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = login(&app).await;
    let response = test::call_service(&app, get("/api/tasks?category=snapshot", &cookies)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(response).await;
    let item = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "list-failed")
        .unwrap();
    assert_eq!(item["dbTypeTarget"], "pg");
    assert_eq!(item["latestRun"]["id"], "run-list-failed");
    assert_eq!(item["latestRun"]["status"], "failed");
    assert_eq!(item["latestRun"]["currentPhase"], "snapshot");
    assert!(item["progress"].is_null());
}

#[actix_web::test]
async fn list_tasks_includes_runtime_progress() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "list-progress", "snapshot").await;
    seed_run(&pool, "list-progress", "running", None).await;
    metric(&pool, "list-progress", "progress", 42.0).await;
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = login(&app).await;
    let response = test::call_service(&app, get("/api/tasks?category=snapshot", &cookies)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(response).await;
    let item = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "list-progress")
        .unwrap();
    assert_eq!(item["latestRun"]["status"], "running");
    assert_eq!(item["progress"]["percent"], 42.0);
}

#[actix_web::test]
async fn detail_separates_snapshot_run_metrics_and_progress() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "snapshot", "snapshot").await;
    seed_run(&pool, "snapshot", "running", None).await;
    metric(&pool, "snapshot", "progress", 42.0).await;
    metric(&pool, "snapshot", "extractor_rps_avg", 9.0).await;
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = login(&app).await;
    let response = test::call_service(&app, get("/api/tasks/snapshot/detail", &cookies)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["task"]["configuredExtractType"], "snapshot");
    assert_eq!(body["currentRun"]["status"], "running");
    assert_eq!(body["currentRun"]["currentPhase"], "snapshot");
    assert_eq!(body["phases"]["snapshot"]["status"], "running");
    assert_eq!(body["metricsSnapshot"]["runId"], "run-snapshot");
    assert_eq!(body["metricsSnapshot"]["values"]["extractor_rps_avg"], 9.0);
    assert_eq!(body["progress"]["percent"], 42.0);
}

#[actix_web::test]
async fn detail_attributes_two_phase_metrics_to_running_cdc_without_snapshot_percent() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "two-phase", "snapshot_and_cdc").await;
    let dir = std::env::temp_dir().join(format!("task-detail-phase-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("phase_state.json"), r#"{"current_phase":2,"start_time_utc":"2026-07-18 00:00:00.000","start_scn":null,"phase2_ini_path":"phase2.ini"}"#).unwrap();
    let log_dir = dir.join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    seed_run(
        &pool,
        "two-phase",
        "running",
        Some(log_dir.to_string_lossy().into()),
    )
    .await;
    metric(&pool, "two-phase", "progress", 100.0).await;
    metric(&pool, "two-phase", "sinker_sinked_records", 7.0).await;
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = login(&app).await;
    let response = test::call_service(&app, get("/api/tasks/two-phase/detail", &cookies)).await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["currentRun"]["currentPhase"], "cdc");
    assert_eq!(body["currentRun"]["status"], "running");
    assert_eq!(body["phases"]["snapshot"]["status"], "completed");
    assert_eq!(body["phases"]["cdc"]["status"], "running");
    assert_eq!(body["metricsSnapshot"]["phase"], "cdc");
    assert!(body["progress"]["percent"].is_null());
}

#[actix_web::test]
async fn detail_reports_terminal_phase_and_missing_runtime_truth() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "stopped", "cdc").await;
    seed_run(&pool, "stopped", "stopped", None).await;
    seed_task(&pool, "failed", "snapshot").await;
    seed_run(&pool, "failed", "failed", None).await;
    seed_task(&pool, "never-run", "snapshot").await;
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = login(&app).await;
    for (task, phase, state) in [
        ("stopped", "cdc", "completed"),
        ("failed", "snapshot", "failed"),
    ] {
        let response =
            test::call_service(&app, get(&format!("/api/tasks/{task}/detail"), &cookies)).await;
        let body: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(body["currentRun"]["status"], task);
        assert_eq!(body["phases"][phase]["status"], state);
        assert!(body["metricsSnapshot"].is_null());
        assert!(body["progress"].is_null());
    }
    let response = test::call_service(&app, get("/api/tasks/never-run/detail", &cookies)).await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert!(body["currentRun"].is_null());
    assert!(body["metricsSnapshot"].is_null());
    assert!(body["progress"].is_null());
}

#[actix_web::test]
async fn detail_scopes_metrics_to_both_task_and_run() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "snapshot", "snapshot").await;
    seed_run(&pool, "snapshot", "running", None).await;
    metric_for_run(
        &pool,
        "snapshot",
        "run-snapshot",
        "extractor_rps_avg",
        "2026-07-18T00:01:00.000Z",
        9.0,
    )
    .await;
    metric_for_run(
        &pool,
        "different-task",
        "run-snapshot",
        "extractor_rps_avg",
        "2026-07-18T00:02:00.000Z",
        99.0,
    )
    .await;

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = login(&app).await;
    let response = test::call_service(&app, get("/api/tasks/snapshot/detail", &cookies)).await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["metricsSnapshot"]["values"]["extractor_rps_avg"], 9.0);
}

#[actix_web::test]
async fn detail_uses_monitor_log_when_metric_points_are_missing() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "monitor-fallback", "snapshot_and_cdc").await;
    let dir = std::env::temp_dir().join(format!("task-detail-monitor-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("phase_state.json"), r#"{"current_phase":2,"start_time_utc":"2026-07-18 00:00:00.000","start_scn":null,"phase2_ini_path":"phase2.ini"}"#).unwrap();
    let log_dir = dir.join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    std::fs::write(
        log_dir.join("monitor.log"),
        "2026-08-01 07:51:07.822095 | pipeline |  | queued_records | latest=3\n2026-08-01 07:51:07.822136 | pipeline |  | sinked_records | latest=65\n",
    )
    .unwrap();
    seed_run(
        &pool,
        "monitor-fallback",
        "running",
        Some(log_dir.to_string_lossy().into()),
    )
    .await;

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = login(&app).await;
    let response =
        test::call_service(&app, get("/api/tasks/monitor-fallback/detail", &cookies)).await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["currentRun"]["currentPhase"], "cdc");
    assert_eq!(body["metricsSnapshot"]["phase"], "cdc");
    assert_eq!(
        body["metricsSnapshot"]["values"]["pipeline_queue_size"],
        3.0
    );
    assert_eq!(
        body["metricsSnapshot"]["values"]["sinker_sinked_records"],
        65.0
    );
    assert_eq!(body["progress"]["copiedRecords"], 65.0);
}

#[actix_web::test]
async fn detail_unknown_task_returns_diagnostic_envelope() {
    let pool = test_pool().await;
    setup(&pool).await;
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = login(&app).await;
    let response = test::call_service(&app, get("/api/tasks/missing/detail", &cookies)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["code"], "TASK_NOT_FOUND");
    assert_eq!(body["details"]["id"], "missing");
}
