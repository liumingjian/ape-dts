//! Integration tests for GET /api/runs/:id/metrics/latest.
//!
//! VAL-ORCH-018: 404 for unknown run id.
//! VAL-ORCH-019: 200 + {} when run exists but no metric_points yet.
//! VAL-ORCH-020: 200 + JSON map with latest values after scrape.
//! VAL-ORCH-021: each metric appears exactly once.
//! VAL-ORCH-022: newer row updates the returned value.
//! VAL-ORCH-036: lag key absent (not null, not 0) when no lag points exist.

use actix_session::storage::CookieSessionStore;
use actix_session::SessionMiddleware;
use actix_web::cookie::{Cookie, Key, SameSite};
use actix_web::http::StatusCode;
use actix_web::test;
use actix_web::web::{self, JsonConfig};
use actix_web::App;
use dt_console_server::auth;
use dt_console_server::db;
use dt_console_server::error;
use dt_console_server::log_sse_handlers;
use dt_console_server::metrics_scraper;
use dt_console_server::middleware::csrf::{Csrf, XSRF_COOKIE_NAME, XSRF_HEADER_NAME};
use dt_console_server::models::{LoginRequest, MetricPoint, ResourceGroup, Run};
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use dt_console_server::repositories::license_repository::LicenseRepository;
use dt_console_server::repositories::metric_point_repository::MetricPointRepository;
use dt_console_server::repositories::resource_group_repository::ResourceGroupRepository;
use dt_console_server::repositories::run_repository::RunRepository;
use dt_console_server::run_handlers;
use sqlx::SqlitePool;

// ─── Test infrastructure ────────────────────────────────────────────────────

async fn test_pool() -> SqlitePool {
    let dir = std::env::temp_dir().join("dt-console-server-metrics-latest-test");
    std::fs::create_dir_all(&dir).unwrap();
    let test_name = std::thread::current()
        .name()
        .unwrap_or("unknown")
        .to_string();
    let safe_name: String = test_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let path = dir.join(format!("ml-{safe_name}.db"));
    let _ = std::fs::remove_file(&path);
    let path_str = path.to_string_lossy().to_string();
    let pool = db::create_pool(&path_str).await.unwrap();
    db::run_migrations(&pool).await.unwrap();
    pool
}

const IDLE_TIMEOUT_SECS: i64 = 3600;
const XSRF: &str = "test-xsrf-token";

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
    let master = key.master().to_vec();
    let key2 = Key::from(&master);

    let session_mw = SessionMiddleware::builder(CookieSessionStore::default(), key2)
        .cookie_name("session".to_string())
        .cookie_secure(false)
        .cookie_http_only(true)
        .cookie_same_site(SameSite::Lax)
        .cookie_path("/".to_string())
        .build();

    App::new()
        .wrap(session_mw)
        .wrap(Csrf)
        .app_data(JsonConfig::default().error_handler(|err, _req| {
            error::ApiError::new(error::codes::PARSE_ERROR, err.to_string()).into()
        }))
        .app_data(web::Data::new(pool))
        .app_data(web::Data::new(RateLimiter::new(RateLimitConfig::default())))
        .app_data(web::Data::new(IDLE_TIMEOUT_SECS))
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

fn collect_cookies<B>(res: &actix_web::dev::ServiceResponse<B>) -> Vec<Cookie<'static>> {
    let mut cookies = Vec::new();
    for val in res.headers().get_all("set-cookie") {
        if let Ok(cookie) = Cookie::parse_encoded(val.to_str().unwrap_or("").to_string()) {
            cookies.push(cookie.into_owned());
        }
    }
    cookies
}

macro_rules! do_login {
    ($app:expr) => {{
        let req = test::TestRequest::post()
            .uri("/api/auth/login")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(LoginRequest {
                username: "admin".to_string(),
                password: "admin123".to_string(),
            })
            .to_request();
        let res = test::call_service(&$app, req).await;
        assert_eq!(res.status(), StatusCode::OK);
        collect_cookies(&res)
    }};
}

fn auth_get(uri: &str, cookies: &[Cookie<'static>]) -> test::TestRequest {
    let mut req = test::TestRequest::get().uri(uri);
    for c in cookies {
        req = req.cookie(c.clone());
    }
    req = req.cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF));
    req = req.insert_header((XSRF_HEADER_NAME, XSRF));
    req
}

async fn setup(pool: &SqlitePool) {
    auth::seed_admin(pool).await.unwrap();
    seed_license(pool).await;
    seed_resource_group(pool).await;
}

async fn seed_resource_group(pool: &SqlitePool) {
    let existing = ResourceGroupRepository::list(pool).await.unwrap();
    if !existing.is_empty() {
        return;
    }
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let rg = ResourceGroup {
        id: "rg-default".to_string(),
        name: "default".to_string(),
        is_default: true,
        created_at: now.clone(),
        updated_at: now,
    };
    ResourceGroupRepository::create(pool, &rg).await.unwrap();
}

async fn seed_license(pool: &SqlitePool) {
    if LicenseRepository::get_current(pool)
        .await
        .unwrap()
        .is_some()
    {
        return;
    }
    use sha2::{Digest, Sha256};
    let max_tasks = 100i64;
    let expire_at = "2030-12-31T23:59:59Z";
    let mut hasher = Sha256::new();
    hasher.update(format!(
        "pro:{max_tasks}:{expire_at}:test-corp:ape-dts-console-license-secret-2025"
    ));
    let sig = format!("{:x}", hasher.finalize())[..16].to_string();
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let license = dt_console_server::models::License {
        id: uuid::Uuid::new_v4().to_string(),
        sku: "pro".to_string(),
        max_tasks,
        expire_at: Some(expire_at.to_string()),
        activated_at: Some(now.clone()),
        activation_code_hash: Some(sig),
        granted_to: "test-corp".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };
    LicenseRepository::create(pool, &license).await.unwrap();
}

async fn seed_task(pool: &SqlitePool, task_id: &str) {
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
    .bind("rg-default")
    .bind("draft")
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_run(pool: &SqlitePool, run_id: &str, task_id: &str) {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let run = Run {
        id: run_id.to_string(),
        task_id: Some(task_id.to_string()),
        status: "running".to_string(),
        pid: Some(1234),
        ini_path: None,
        log_dir: None,
        started_at: Some(now.clone()),
        stopped_at: None,
        exit_code: None,
        stop_method: None,
        metrics_port: None,
        created_at: now.clone(),
        updated_at: now,
    };
    RunRepository::create(pool, &run).await.unwrap();
}

async fn insert_metric_point(
    pool: &SqlitePool,
    run_id: &str,
    task_id: &str,
    metric_name: &str,
    value: f64,
    ts: &str,
) {
    let mp = MetricPoint {
        id: 0,
        task_id: task_id.to_string(),
        run_id: run_id.to_string(),
        metric_name: metric_name.to_string(),
        ts: ts.to_string(),
        value,
    };
    MetricPointRepository::create(pool, &mp).await.unwrap();
}

// ─── Tests ──────────────────────────────────────────────────────────────────

/// VAL-ORCH-018: unknown run id returns 404 with RUN_NOT_FOUND envelope.
#[actix_web::test]
async fn metrics_latest_unknown_run_returns_404() {
    let pool = test_pool().await;
    setup(&pool).await;
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/nonexistent-run-id/metrics/latest", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], "RUN_NOT_FOUND");
    assert!(body["details"].is_object());
}

/// VAL-ORCH-019: run exists but no metric_points → 200 with literal {}.
#[actix_web::test]
async fn metrics_latest_no_points_returns_empty_object() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "task-no-pts").await;
    seed_run(&pool, "run-no-pts", "task-no-pts").await;
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-no-pts/metrics/latest", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert!(body.is_object());
    assert_eq!(
        body.as_object().unwrap().len(),
        0,
        "expected empty JSON object"
    );
}

/// VAL-ORCH-020: after scrape, returns JSON map with latest values per metric.
#[actix_web::test]
async fn metrics_latest_returns_latest_values_after_scrape() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "task-scrape").await;
    seed_run(&pool, "run-scrape", "task-scrape").await;

    // Insert two metrics with multiple data points each.
    insert_metric_point(
        &pool,
        "run-scrape",
        "task-scrape",
        "progress",
        50.0,
        "2026-01-01T00:00:00.000Z",
    )
    .await;
    insert_metric_point(
        &pool,
        "run-scrape",
        "task-scrape",
        "progress",
        75.0,
        "2026-01-01T00:00:10.000Z",
    )
    .await;
    insert_metric_point(
        &pool,
        "run-scrape",
        "task-scrape",
        "pipeline_queue_size",
        12.0,
        "2026-01-01T00:00:00.000Z",
    )
    .await;
    insert_metric_point(
        &pool,
        "run-scrape",
        "task-scrape",
        "pipeline_queue_size",
        8.0,
        "2026-01-01T00:00:10.000Z",
    )
    .await;

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-scrape/metrics/latest", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert!(body.is_object());
    // latest progress = 75.0 (from ts 00:00:10)
    assert_eq!(body["progress"].as_f64().unwrap(), 75.0);
    // latest pipeline_queue_size = 8.0 (from ts 00:00:10)
    assert_eq!(body["pipeline_queue_size"].as_f64().unwrap(), 8.0);
}

/// VAL-ORCH-021: each metric appears exactly once (unique keys).
#[actix_web::test]
async fn metrics_latest_keys_are_unique() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "task-unique").await;
    seed_run(&pool, "run-unique", "task-unique").await;

    // Insert 3 data points for the same metric at different times.
    insert_metric_point(
        &pool,
        "run-unique",
        "task-unique",
        "lag",
        10.0,
        "2026-01-01T00:00:00.000Z",
    )
    .await;
    insert_metric_point(
        &pool,
        "run-unique",
        "task-unique",
        "lag",
        5.0,
        "2026-01-01T00:00:05.000Z",
    )
    .await;
    insert_metric_point(
        &pool,
        "run-unique",
        "task-unique",
        "lag",
        3.0,
        "2026-01-01T00:00:10.000Z",
    )
    .await;
    insert_metric_point(
        &pool,
        "run-unique",
        "task-unique",
        "progress",
        50.0,
        "2026-01-01T00:00:00.000Z",
    )
    .await;

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-unique/metrics/latest", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    let obj = body.as_object().unwrap();
    // Exactly 2 distinct keys: lag and progress.
    assert_eq!(obj.len(), 2);
    // lag should be the latest value (3.0 at 00:00:10).
    assert_eq!(body["lag"].as_f64().unwrap(), 3.0);
    // progress should be 50.0.
    assert_eq!(body["progress"].as_f64().unwrap(), 50.0);
}

/// VAL-ORCH-022: inserting a newer row updates the returned value for that metric.
#[actix_web::test]
async fn metrics_latest_newer_row_updates_value() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "task-update").await;
    seed_run(&pool, "run-update", "task-update").await;

    insert_metric_point(
        &pool,
        "run-update",
        "task-update",
        "lag",
        10.0,
        "2026-01-01T00:00:00.000Z",
    )
    .await;
    insert_metric_point(
        &pool,
        "run-update",
        "task-update",
        "progress",
        50.0,
        "2026-01-01T00:00:00.000Z",
    )
    .await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app);

    // First call: lag=10.0, progress=50.0
    let req = auth_get("/api/runs/run-update/metrics/latest", &cookies).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["lag"].as_f64().unwrap(), 10.0);
    assert_eq!(body["progress"].as_f64().unwrap(), 50.0);

    // Insert a newer lag point.
    insert_metric_point(
        &pool,
        "run-update",
        "task-update",
        "lag",
        2.0,
        "2026-01-01T00:00:10.000Z",
    )
    .await;

    // Second call: lag should update to 2.0; progress unchanged.
    let req = auth_get("/api/runs/run-update/metrics/latest", &cookies).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["lag"].as_f64().unwrap(), 2.0);
    assert_eq!(body["progress"].as_f64().unwrap(), 50.0);
}

/// VAL-ORCH-036: lag key absent when no lag points exist (not null, not 0).
#[actix_web::test]
async fn metrics_latest_lag_absent_when_no_lag_points() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "task-no-lag").await;
    seed_run(&pool, "run-no-lag", "task-no-lag").await;

    // Insert non-lag metrics only.
    insert_metric_point(
        &pool,
        "run-no-lag",
        "task-no-lag",
        "progress",
        30.0,
        "2026-01-01T00:00:00.000Z",
    )
    .await;
    insert_metric_point(
        &pool,
        "run-no-lag",
        "task-no-lag",
        "pipeline_queue_size",
        5.0,
        "2026-01-01T00:00:00.000Z",
    )
    .await;

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-no-lag/metrics/latest", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    // lag key must be absent — not null, not 0.
    assert!(
        !body.as_object().unwrap().contains_key("lag"),
        "lag key must be absent, got: {body}"
    );
    // Other metrics are present.
    assert!(body.as_object().unwrap().contains_key("progress"));
    assert!(body
        .as_object()
        .unwrap()
        .contains_key("pipeline_queue_size"));
}

/// Legacy metric_points rows with metric_name='delay' must not crash the endpoint.
#[actix_web::test]
async fn metrics_latest_delay_rows_do_not_crash() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task(&pool, "task-delay").await;
    seed_run(&pool, "run-delay", "task-delay").await;

    // Insert pre-existing 'delay' rows (simulating legacy data).
    insert_metric_point(
        &pool,
        "run-delay",
        "task-delay",
        "delay",
        5.0,
        "2026-01-01T00:00:00.000Z",
    )
    .await;
    insert_metric_point(
        &pool,
        "run-delay",
        "task-delay",
        "lag",
        3.0,
        "2026-01-01T00:00:00.000Z",
    )
    .await;

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-delay/metrics/latest", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    // Must return 200, not 500 — regardless of whether 'delay' appears in the response.
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert!(body.is_object(), "response must be a JSON object");
    // lag must be present.
    assert_eq!(body["lag"].as_f64().unwrap(), 3.0);
}
