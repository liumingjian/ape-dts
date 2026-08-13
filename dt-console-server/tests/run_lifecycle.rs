//! Integration tests for Run lifecycle endpoints and LocalExecutor.
//!
//! Tests cover the core run lifecycle behaviours:
//! - Start a stopped Task → 202 with run_id
//! - Start already-running → 409
//! - Stop running → 202
//! - Pause/Resume transitions → 202/409 as appropriate
//! - Sequential Runs produce isolated cwds
//! - Run position reading (null when missing)
//! - Run inspection (GET /api/runs/:id)
//! - RBAC enforcement on lifecycle endpoints
//! - Control logs written on start/stop
//! - RG reassignment blocked during active Run
//! - State machine legality

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
use dt_console_server::executor;
use dt_console_server::log_sse_handlers;
use dt_console_server::metrics_scraper;
use dt_console_server::middleware::csrf::{Csrf, XSRF_COOKIE_NAME, XSRF_HEADER_NAME};
use dt_console_server::models::{LoginRequest, ResourceGroup};
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use dt_console_server::repositories::license_repository::LicenseRepository;
use dt_console_server::repositories::resource_group_repository::ResourceGroupRepository;
use dt_console_server::repositories::run_repository::RunRepository;
use dt_console_server::run_handlers;
use sqlx::SqlitePool;
use std::path::PathBuf;

async fn test_pool() -> SqlitePool {
    let dir = std::env::temp_dir().join("dt-console-server-run-test");
    std::fs::create_dir_all(&dir).unwrap();
    let test_name = std::thread::current()
        .name()
        .unwrap_or("unknown")
        .to_string();
    let safe_name: String = test_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let path = dir.join(format!("run-{safe_name}.db"));
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
    active_runs: run_handlers::ActiveRuns,
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
        .app_data(web::Data::new(active_runs))
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
    ($app:expr, $username:expr, $password:expr) => {{
        let req = test::TestRequest::post()
            .uri("/api/auth/login")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(LoginRequest {
                username: $username.to_string(),
                password: $password.to_string(),
            })
            .to_request();
        let res = test::call_service(&$app, req).await;
        assert_eq!(res.status(), StatusCode::OK);
        collect_cookies(&res)
    }};
}

fn add_auth(mut req: test::TestRequest, cookies: &[Cookie<'static>]) -> test::TestRequest {
    for c in cookies {
        req = req.cookie(c.clone());
    }
    req = req.cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF));
    req = req.insert_header((XSRF_HEADER_NAME, XSRF));
    req
}

async fn setup() -> (SqlitePool, run_handlers::ActiveRuns) {
    let pool = test_pool().await;
    auth::seed_admin(&pool).await.unwrap();
    seed_default_rg(&pool).await;
    activate_license(&pool, 100).await;
    let active_runs = run_handlers::new_active_runs();
    (pool, active_runs)
}

async fn seed_default_rg(pool: &SqlitePool) {
    let existing = ResourceGroupRepository::list(pool).await.unwrap();
    if !existing.is_empty() {
        return;
    }
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let rg = ResourceGroup {
        id: uuid::Uuid::new_v4().to_string(),
        name: "default".to_string(),
        is_default: true,
        created_at: now.clone(),
        updated_at: now,
    };
    ResourceGroupRepository::create(pool, &rg).await.unwrap();
}

async fn activate_license(pool: &SqlitePool, max_tasks: i64) {
    let existing = LicenseRepository::get_current(pool).await.unwrap();
    if existing.is_some() {
        return;
    }
    use sha2::{Digest, Sha256};
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

async fn cleanup_run_dirs(pool: &SqlitePool, task_id: &str) {
    let runs = RunRepository::list_by_task(pool, task_id).await.unwrap();
    let base_dir = executor::run_data_dir();
    for run in runs {
        let _ = std::fs::remove_dir_all(PathBuf::from(&base_dir).join(&run.id));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// STATE MACHINE UNIT TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_legal_transitions() {
    assert!(dt_console_server::models::is_legal_transition(
        "pending", "running"
    ));
    assert!(dt_console_server::models::is_legal_transition(
        "running", "pausing"
    ));
    assert!(dt_console_server::models::is_legal_transition(
        "pausing", "paused"
    ));
    assert!(dt_console_server::models::is_legal_transition(
        "pausing", "failed"
    ));
    assert!(dt_console_server::models::is_legal_transition(
        "paused", "running"
    ));
    assert!(dt_console_server::models::is_legal_transition(
        "paused", "stopped"
    ));
    // A pause that will not converge must still be stoppable, or its task is
    // frozen by `is_active` with nothing able to move it.
    assert!(dt_console_server::models::is_legal_transition(
        "pausing", "stopping"
    ));
    assert!(dt_console_server::models::is_legal_transition(
        "pausing", "running"
    ));
    assert!(dt_console_server::models::is_legal_transition(
        "running", "stopping"
    ));
    assert!(dt_console_server::models::is_legal_transition(
        "paused", "stopping"
    ));
    assert!(dt_console_server::models::is_legal_transition(
        "stopping", "stopped"
    ));
    assert!(dt_console_server::models::is_legal_transition(
        "stopping", "failed"
    ));
    assert!(dt_console_server::models::is_legal_transition(
        "running", "failed"
    ));
    assert!(dt_console_server::models::is_legal_transition(
        "pending", "failed"
    ));
}

#[tokio::test]
async fn test_illegal_transitions() {
    assert!(!dt_console_server::models::is_legal_transition(
        "stopped", "running"
    ));
    assert!(!dt_console_server::models::is_legal_transition(
        "failed", "running"
    ));
    assert!(!dt_console_server::models::is_legal_transition(
        "stopped", "paused"
    ));
    assert!(!dt_console_server::models::is_legal_transition(
        "paused", "paused"
    ));
    assert!(!dt_console_server::models::is_legal_transition(
        "running", "running"
    ));
    assert!(!dt_console_server::models::is_legal_transition(
        "pending", "stopped"
    ));
    assert!(!dt_console_server::models::is_legal_transition(
        "stopped", "failed"
    ));
    assert!(!dt_console_server::models::is_legal_transition(
        "failed", "stopped"
    ));
    assert!(!dt_console_server::models::is_legal_transition(
        "failed", "paused"
    ));
    assert!(!dt_console_server::models::is_legal_transition(
        "stopped", "stopping"
    ));
    // Pause is a graceful stop, not a suspend: `running` goes to `pausing`
    // and only the supervisor, reading the real exit code, may write
    // `paused`.
    assert!(!dt_console_server::models::is_legal_transition(
        "running", "paused"
    ));
}

#[tokio::test]
async fn test_run_status_helpers() {
    use dt_console_server::models::run_status;
    assert!(run_status::is_active("pending"));
    assert!(run_status::is_active("running"));
    assert!(run_status::is_active("pausing"));
    assert!(run_status::is_active("paused"));
    assert!(run_status::is_active("stopping"));
    assert!(!run_status::is_active("stopped"));
    assert!(!run_status::is_active("failed"));
    assert!(run_status::is_terminal("stopped"));
    assert!(run_status::is_terminal("failed"));
    assert!(!run_status::is_terminal("running"));
}

// ═══════════════════════════════════════════════════════════════════════════
// EXECUTOR UNIT TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_executor_spawn_and_kill() {
    let run_id = format!("itest-{}", uuid::Uuid::new_v4());
    let ini_content = "[global]\ntask_id=test\n";

    let handle = executor::LocalExecutor::spawn(&run_id, ini_content, Some("sleep"))
        .await
        .unwrap();
    assert!(handle.pid > 0);
    assert!(handle.run_dir.exists());
    assert!(handle.run_dir.join("task_config.ini").exists());
    assert!(handle.run_dir.join("logs").exists());

    let status = executor::LocalExecutor::status(&handle).await;
    assert!(matches!(status, executor::ChildStatus::Running));

    let result = executor::LocalExecutor::kill_with_grace(&handle, 3).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().stop_method, "sigterm");
    let _ = std::fs::remove_dir_all(&handle.run_dir);
}

#[tokio::test]
async fn test_executor_binary_path_override() {
    let run_id = format!("itest-override-{}", uuid::Uuid::new_v4());
    let handle = executor::LocalExecutor::spawn(&run_id, "[global]\ntask_id=ov\n", Some("sleep"))
        .await
        .unwrap();
    assert!(handle.pid > 0);
    let _ = executor::LocalExecutor::kill_with_grace(&handle, 2).await;
    let _ = std::fs::remove_dir_all(&handle.run_dir);
}

#[tokio::test]
async fn test_executor_failed_run_records_nonzero_exit() {
    let run_id = format!("itest-fail-{}", uuid::Uuid::new_v4());
    let handle = executor::LocalExecutor::spawn(&run_id, "[global]\ntask_id=f\n", Some("false"))
        .await
        .unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let status = executor::LocalExecutor::status(&handle).await;
    match status {
        executor::ChildStatus::Exited(executor::ExitStatus::Exited { code }) => {
            assert_ne!(code, 0);
        }
        other => panic!("expected Exited non-zero, got {:?}", other),
    }
    let _ = std::fs::remove_dir_all(&handle.run_dir);
}

#[tokio::test]
async fn test_executor_spawn_writes_ini() {
    let run_id = format!("itest-ini-{}", uuid::Uuid::new_v4());
    let ini_content = "[global]\ntask_id=ini_test\n[extractor]\ndb_type=mysql\n";
    let handle = executor::LocalExecutor::spawn(&run_id, ini_content, Some("sleep"))
        .await
        .unwrap();
    let content = std::fs::read_to_string(handle.run_dir.join("task_config.ini")).unwrap();
    assert_eq!(content, ini_content);
    let _ = executor::LocalExecutor::kill_with_grace(&handle, 2).await;
    let _ = std::fs::remove_dir_all(&handle.run_dir);
}

#[tokio::test]
async fn test_position_reading_missing_file() {
    let dir = std::env::temp_dir().join(format!("test-pos-miss-{}", uuid::Uuid::new_v4()));
    let result = executor::LocalExecutor::read_position(&dir);
    assert!(result.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_position_reading_with_content() {
    let dir = std::env::temp_dir().join(format!("test-pos-ok-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("logs")).unwrap();
    std::fs::write(dir.join("logs/position.log"), "lsn=0/1A2B3C4D\n").unwrap();
    let result = executor::LocalExecutor::read_position(&dir);
    assert!(result.is_some());
    assert_eq!(result.unwrap()["lsn"], "0/1A2B3C4D");
    let _ = std::fs::remove_dir_all(&dir);
}

// ═══════════════════════════════════════════════════════════════════════════
// HTTP LIFECYCLE INTEGRATION TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[actix_web::test]
async fn test_start_task_returns_202_with_run_id() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create a task.
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    std::env::set_var("APE_DTS_BINARY_PATH", "sleep");

    // Start the task.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::ACCEPTED,
        "start should return 202"
    );

    let body: serde_json::Value = test::read_body_json(resp).await;
    let run_id = body["runId"].as_str().unwrap();
    assert!(!run_id.is_empty(), "runId should be non-empty");

    // Verify run record exists in DB.
    let run = RunRepository::find_by_id(&pool, run_id).await.unwrap();
    assert_eq!(run.status, "running");
    assert!(run.pid.is_some());
    assert!(run.pid.unwrap() > 0);

    // Clean up.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    std::env::remove_var("APE_DTS_BINARY_PATH");
    cleanup_run_dirs(&pool, &task_id).await;
}

#[actix_web::test]
async fn test_start_already_running_returns_409() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    std::env::set_var("APE_DTS_BINARY_PATH", "sleep");

    // First start.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // Second start should fail.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "RUN_ALREADY_ACTIVE");

    // Clean up.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    std::env::remove_var("APE_DTS_BINARY_PATH");
    cleanup_run_dirs(&pool, &task_id).await;
}

#[actix_web::test]
async fn test_stop_not_active_returns_409() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    // Stop without a running Run should return 409 ILLEGAL_TRANSITION.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "ILLEGAL_TRANSITION");
    // Details should contain from/to transition info.
    assert!(body["details"]["from"].is_string());
    assert!(body["details"]["to"].is_string());
}

#[actix_web::test]
async fn test_pause_not_running_returns_409() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/pause")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = test::read_body_json(resp).await;
    // When there's no active run, the handler returns ILLEGAL_TRANSITION with {from, to}.
    assert_eq!(body["code"], "ILLEGAL_TRANSITION");
    assert!(body["details"]["from"].is_string());
    assert_eq!(body["details"]["to"], "paused");
}

#[actix_web::test]
async fn test_resume_not_paused_returns_409() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/resume")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = test::read_body_json(resp).await;
    // When there's no active run, the handler returns ILLEGAL_TRANSITION with {from, to}.
    assert_eq!(body["code"], "ILLEGAL_TRANSITION");
    assert!(body["details"]["from"].is_string());
    assert_eq!(body["details"]["to"], "running");
}

#[actix_web::test]
async fn test_get_run_returns_details() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    std::env::set_var("APE_DTS_BINARY_PATH", "sleep");

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    let run_id = body["runId"].as_str().unwrap().to_string();

    // Get run details.
    let req = add_auth(
        test::TestRequest::get().uri(&format!("/api/runs/{run_id}")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["id"], run_id);
    assert_eq!(body["taskId"], task_id);
    assert_eq!(body["status"], "running");
    assert!(body["pid"].is_number());

    // Clean up.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    std::env::remove_var("APE_DTS_BINARY_PATH");
    cleanup_run_dirs(&pool, &task_id).await;
}

#[actix_web::test]
async fn test_get_run_not_found_returns_404() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::get().uri("/api/runs/nonexistent-id"),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let status = resp.status();
    let body: serde_json::Value = test::read_body_json(resp).await;
    if status != StatusCode::NOT_FOUND {
        eprintln!("Expected 404 but got {status}. Body: {body}");
    }
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "RUN_NOT_FOUND");
}

#[actix_web::test]
async fn test_run_position_null_when_missing() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    std::env::set_var("APE_DTS_BINARY_PATH", "sleep");

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    let run_id = body["runId"].as_str().unwrap().to_string();

    let req = add_auth(
        test::TestRequest::get().uri(&format!("/api/runs/{run_id}")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(
        body["position"].is_null(),
        "position should be null when position.log missing"
    );

    // Clean up.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    std::env::remove_var("APE_DTS_BINARY_PATH");
    cleanup_run_dirs(&pool, &task_id).await;
}

#[actix_web::test]
async fn test_viewer_cannot_start_task() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;

    let admin_cookies = do_login!(app, "admin", "admin123");

    // Create a viewer.
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/users")
            .set_json(serde_json::json!({
                "username": "viewer1",
                "password": "viewer123",
                "role": "viewer",
                "displayName": "Test Viewer"
            })),
        &admin_cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let viewer_cookies = do_login!(app, "viewer1", "viewer123");

    // Create a task with admin.
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &admin_cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    // Viewer should not be able to start.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &viewer_cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "FORBIDDEN");
}

#[actix_web::test]
async fn test_viewer_cannot_stop_task() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;

    let admin_cookies = do_login!(app, "admin", "admin123");
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/users")
            .set_json(serde_json::json!({
                "username": "viewer2",
                "password": "viewer123",
                "role": "viewer",
                "displayName": "Test Viewer 2"
            })),
        &admin_cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;

    let viewer_cookies = do_login!(app, "viewer2", "viewer123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &admin_cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &viewer_cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[actix_web::test]
async fn test_rg_reassignment_blocked_during_active_run() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create a second resource group.
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/resource_groups")
            .set_json(serde_json::json!({ "name": "team-b" })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let rg_body: serde_json::Value = test::read_body_json(resp).await;
    let new_rg_id = rg_body["id"].as_str().unwrap().to_string();

    // Create a task.
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    std::env::set_var("APE_DTS_BINARY_PATH", "sleep");

    // Start the task.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // Try to reassign RG while running — should fail.
    let req = add_auth(
        test::TestRequest::patch()
            .uri(&format!("/api/tasks/{task_id}"))
            .set_json(serde_json::json!({ "resourceGroupId": new_rg_id })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "TASK_HAS_ACTIVE_RUN");

    // Clean up.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    std::env::remove_var("APE_DTS_BINARY_PATH");
    cleanup_run_dirs(&pool, &task_id).await;
}

#[actix_web::test]
async fn test_sequential_runs_produce_isolated_cwds() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    std::env::set_var("APE_DTS_BINARY_PATH", "sleep");

    // Start Run A.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body_a: serde_json::Value = test::read_body_json(resp).await;
    let run_id_a = body_a["runId"].as_str().unwrap().to_string();

    // Stop Run A.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Start Run B.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let body_b: serde_json::Value = test::read_body_json(resp).await;
    let run_id_b = body_b["runId"].as_str().unwrap().to_string();

    // Run IDs should be different.
    assert_ne!(run_id_a, run_id_b);

    // Verify isolated directories.
    let base_dir = executor::run_data_dir();
    let dir_a = PathBuf::from(&base_dir).join(&run_id_a);
    let dir_b = PathBuf::from(&base_dir).join(&run_id_b);
    assert!(dir_a.exists());
    assert!(dir_b.exists());
    assert!(dir_a.join("task_config.ini").exists());
    assert!(dir_b.join("task_config.ini").exists());

    // Clean up.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    std::env::remove_var("APE_DTS_BINARY_PATH");
    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);
}

#[actix_web::test]
async fn test_control_logs_written_on_start_and_stop() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    std::env::set_var("APE_DTS_BINARY_PATH", "sleep");

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    let run_id = body["runId"].as_str().unwrap().to_string();

    // Check control logs for start.
    let logs =
        dt_console_server::repositories::control_log_repository::ControlLogRepository::list(&pool)
            .await
            .unwrap();
    let start_logs: Vec<_> = logs
        .iter()
        .filter(|l| l.action == "start" && l.run_id.as_deref() == Some(&run_id))
        .collect();
    if start_logs.len() < 2 {
        eprintln!(
            "Expected >= 2 start logs, got {}. All logs:",
            start_logs.len()
        );
        for l in &logs {
            eprintln!(
                "  action={} run_id={:?} intent_or_result={}",
                l.action, l.run_id, l.intent_or_result
            );
        }
    }
    assert!(
        start_logs.len() >= 2,
        "should have intent + result logs for start, got {}",
        start_logs.len()
    );

    let has_intent = start_logs.iter().any(|l| l.intent_or_result == "intent");
    let has_result = start_logs
        .iter()
        .any(|l| l.intent_or_result.starts_with("result:"));
    assert!(has_intent);
    assert!(has_result);

    // Stop.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Check control logs for stop.
    let logs =
        dt_console_server::repositories::control_log_repository::ControlLogRepository::list(&pool)
            .await
            .unwrap();
    let stop_logs: Vec<_> = logs
        .iter()
        .filter(|l| l.action == "stop" && l.run_id.as_deref() == Some(&run_id))
        .collect();
    assert!(
        stop_logs.len() >= 2,
        "should have intent + result logs for stop, got {}",
        stop_logs.len()
    );

    std::env::remove_var("APE_DTS_BINARY_PATH");
    cleanup_run_dirs(&pool, &task_id).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// CONTROL LOG LIFECYCLE AUDIT TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// VAL-CTRL-001: start writes intent + result rows in order, ts monotonically non-decreasing.
#[actix_web::test]
async fn test_control_log_start_intent_then_result_in_order() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    std::env::set_var("APE_DTS_BINARY_PATH", "sleep");

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let run_id = body["runId"].as_str().unwrap().to_string();

    // Verify start intent + result in DB, ordered by ts.
    let logs =
        dt_console_server::repositories::control_log_repository::ControlLogRepository::list(&pool)
            .await
            .unwrap();
    let start_logs: Vec<_> = logs
        .iter()
        .filter(|l| l.action == "start" && l.run_id.as_deref() == Some(&run_id))
        .collect();
    assert_eq!(
        start_logs.len(),
        2,
        "should have exactly 2 start logs (intent + result)"
    );

    let intent = start_logs
        .iter()
        .find(|l| l.intent_or_result == "intent")
        .unwrap();
    let result = start_logs
        .iter()
        .find(|l| l.intent_or_result.starts_with("result:"))
        .unwrap();

    // Intent must come before result (created_at monotonic).
    assert!(
        intent.created_at <= result.created_at,
        "intent ts ({}) must be <= result ts ({})",
        intent.created_at,
        result.created_at
    );
    assert!(
        result.intent_or_result.starts_with("result:success"),
        "result should be success"
    );

    // Verify operator_id is set.
    assert_eq!(intent.operator_id.as_deref(), Some("admin"));
    assert_eq!(result.operator_id.as_deref(), Some("admin"));

    // Clean up.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    std::env::remove_var("APE_DTS_BINARY_PATH");
    cleanup_run_dirs(&pool, &task_id).await;
}

/// VAL-CTRL-002: stop writes intent + result rows.
#[actix_web::test]
async fn test_control_log_stop_intent_and_result() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    std::env::set_var("APE_DTS_BINARY_PATH", "sleep");

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    let run_id = body["runId"].as_str().unwrap().to_string();

    // Stop.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Verify stop intent + result.
    let logs =
        dt_console_server::repositories::control_log_repository::ControlLogRepository::list(&pool)
            .await
            .unwrap();
    let stop_logs: Vec<_> = logs
        .iter()
        .filter(|l| l.action == "stop" && l.run_id.as_deref() == Some(&run_id))
        .collect();
    assert_eq!(stop_logs.len(), 2, "should have exactly 2 stop logs");

    let has_intent = stop_logs.iter().any(|l| l.intent_or_result == "intent");
    let has_result = stop_logs
        .iter()
        .any(|l| l.intent_or_result.starts_with("result:"));
    assert!(has_intent, "should have a stop intent log");
    assert!(has_result, "should have a stop result log");

    std::env::remove_var("APE_DTS_BINARY_PATH");
    cleanup_run_dirs(&pool, &task_id).await;
}

/// VAL-CTRL-004: Intent row survives an aborted action (no result row).
#[actix_web::test]
async fn test_control_log_intent_survives_aborted_action() {
    let pool = test_pool().await;
    dt_console_server::auth::seed_admin(&pool).await.unwrap();
    seed_default_rg(&pool).await;
    activate_license(&pool, 100).await;

    // Create a real task so the FK constraint is satisfied.
    let rg_list =
        dt_console_server::repositories::resource_group_repository::ResourceGroupRepository::list(
            &pool,
        )
        .await
        .unwrap();
    let rg_id = rg_list[0].id.clone();
    let task_id = uuid::Uuid::new_v4().to_string();
    let now_ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let task = dt_console_server::models::Task {
        id: task_id.clone(),
        task_id: "test-orphan-task".to_string(),
        name: "Test Orphan".to_string(),
        kind: "snapshot".to_string(),
        db_type_source: "mysql".to_string(),
        db_type_target: "mysql".to_string(),
        source_endpoint: "{}".to_string(),
        target_endpoint: "{}".to_string(),
        extractor_config: "{}".to_string(),
        sinker_config: "{}".to_string(),
        filter_config: "{}".to_string(),
        router_config: "{}".to_string(),
        parallelizer_config: "{}".to_string(),
        pipeline_config: "{}".to_string(),
        resumer_config: "{}".to_string(),
        processor_config: "{}".to_string(),
        runtime_config: "{}".to_string(),
        metrics_config: "{}".to_string(),
        resource_group_id: rg_id,
        owner_user_id: None,
        status: "draft".to_string(),
        created_at: now_ts,
        updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    };
    dt_console_server::repositories::task_repository::TaskRepository::create(&pool, &task)
        .await
        .unwrap();

    // Directly insert an orphaned intent row (simulating a crashed action).
    let run_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let orphan = dt_console_server::models::ControlLog {
        id: 0,
        task_id: task_id.clone(),
        run_id: Some(run_id),
        action: "start".to_string(),
        intent_or_result: "intent".to_string(),
        operator_id: Some("admin".to_string()),
        created_at: now,
    };
    dt_console_server::repositories::control_log_repository::ControlLogRepository::create(
        &pool, &orphan,
    )
    .await
    .unwrap();

    // Verify the intent row exists without a result companion.
    let orphans = dt_console_server::repositories::control_log_repository::ControlLogRepository::find_orphaned_intents(&pool)
        .await
        .unwrap();
    assert_eq!(orphans.len(), 1, "should find exactly 1 orphaned intent");
    assert_eq!(orphans[0].action, "start");
    assert_eq!(orphans[0].intent_or_result, "intent");
    assert_eq!(orphans[0].operator_id.as_deref(), Some("admin"));

    // Verify the full list still returns the orphaned intent.
    let all_logs =
        dt_console_server::repositories::control_log_repository::ControlLogRepository::list(&pool)
            .await
            .unwrap();
    let start_intents: Vec<_> = all_logs
        .iter()
        .filter(|l| l.intent_or_result == "intent" && l.action == "start")
        .collect();
    assert_eq!(start_intents.len(), 1, "intent row should survive");
}

/// VAL-CTRL-005: GET /api/control_logs returns rows ordered by ts desc with filters.
#[actix_web::test]
async fn test_get_control_logs_ordered_by_ts_desc_with_filters() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    std::env::set_var("APE_DTS_BINARY_PATH", "sleep");

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    let _run_id = body["runId"].as_str().unwrap().to_string();

    // GET /api/control_logs with no filters.
    let req = add_auth(test::TestRequest::get().uri("/api/control_logs"), &cookies).to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(
        body["total"].as_i64().unwrap() >= 2,
        "should have at least 2 logs"
    );
    let items = body["items"].as_array().unwrap();
    // Verify ordering: ts desc.
    for i in 1..items.len() {
        let prev = items[i - 1]["ts"].as_str().unwrap();
        let curr = items[i]["ts"].as_str().unwrap();
        assert!(prev >= curr, "control logs should be ordered by ts desc");
    }

    // GET /api/control_logs?taskId=...
    let req = add_auth(
        test::TestRequest::get().uri(&format!("/api/control_logs?taskId={task_id}")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert!(
        items.len() >= 2,
        "filtered by task_id should return at least 2 logs"
    );
    for item in items {
        assert_eq!(item["taskId"].as_str().unwrap(), task_id);
    }

    // GET /api/control_logs?action=start
    let req = add_auth(
        test::TestRequest::get().uri("/api/control_logs?action=start"),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    for item in items {
        assert_eq!(item["action"].as_str().unwrap(), "start");
    }

    // Verify response shape: phase, result fields.
    let intent_item = items
        .iter()
        .find(|i| i["phase"].as_str() == Some("intent"))
        .unwrap();
    assert!(
        intent_item["result"].is_null(),
        "intent should have null result"
    );
    let result_item = items
        .iter()
        .find(|i| i["phase"].as_str() == Some("result"))
        .unwrap();
    assert!(
        result_item["result"].is_string(),
        "result should have a string value"
    );
    assert_eq!(result_item["result"].as_str().unwrap(), "success");

    // Verify operatorId in response.
    assert_eq!(intent_item["operatorId"].as_str().unwrap(), "admin");

    // Clean up.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    std::env::remove_var("APE_DTS_BINARY_PATH");
    cleanup_run_dirs(&pool, &task_id).await;
}

/// VAL-CTRL-005 extension: operator and viewer cannot read control_logs.
#[actix_web::test]
async fn test_control_logs_operator_viewer_forbidden() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let admin_cookies = do_login!(app, "admin", "admin123");

    // Create operator.
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/users")
            .set_json(serde_json::json!({
                "username": "op_ctrl",
                "password": "op123456",
                "role": "operator",
                "displayName": "Op Ctrl"
            })),
        &admin_cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;

    // Create viewer.
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/users")
            .set_json(serde_json::json!({
                "username": "vw_ctrl",
                "password": "vw123456",
                "role": "viewer",
                "displayName": "Vw Ctrl"
            })),
        &admin_cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;

    let op_cookies = do_login!(app, "op_ctrl", "op123456");
    let vw_cookies = do_login!(app, "vw_ctrl", "vw123456");

    // Operator: GET /api/control_logs → 403.
    let req = add_auth(
        test::TestRequest::get().uri("/api/control_logs"),
        &op_cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Viewer: GET /api/control_logs → 403.
    let req = add_auth(
        test::TestRequest::get().uri("/api/control_logs"),
        &vw_cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// VAL-INTEG-010: Orphaned control-log intents are finalised on restart.
#[actix_web::test]
async fn test_orphaned_intents_finalised_on_restart() {
    let pool = test_pool().await;
    dt_console_server::auth::seed_admin(&pool).await.unwrap();
    seed_default_rg(&pool).await;
    activate_license(&pool, 100).await;

    // Create real tasks so FK constraints are satisfied.
    let rg_list =
        dt_console_server::repositories::resource_group_repository::ResourceGroupRepository::list(
            &pool,
        )
        .await
        .unwrap();
    let rg_id = rg_list[0].id.clone();

    let task_id_abc = uuid::Uuid::new_v4().to_string();
    let task_id_xyz = uuid::Uuid::new_v4().to_string();
    for tid in [&task_id_abc, &task_id_xyz] {
        let t = dt_console_server::models::Task {
            id: tid.clone(),
            task_id: format!("test-{tid}"),
            name: "Orphan Test".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "mysql".to_string(),
            db_type_target: "mysql".to_string(),
            source_endpoint: "{}".to_string(),
            target_endpoint: "{}".to_string(),
            extractor_config: "{}".to_string(),
            sinker_config: "{}".to_string(),
            filter_config: "{}".to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: "{}".to_string(),
            pipeline_config: "{}".to_string(),
            resumer_config: "{}".to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: rg_id.clone(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        };
        dt_console_server::repositories::task_repository::TaskRepository::create(&pool, &t)
            .await
            .unwrap();
    }

    // Simulate orphaned intents from a previous orchestrator session.
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let orphan1 = dt_console_server::models::ControlLog {
        id: 0,
        task_id: task_id_abc.clone(),
        run_id: Some("run-1".to_string()),
        action: "start".to_string(),
        intent_or_result: "intent".to_string(),
        operator_id: Some("admin".to_string()),
        created_at: now.clone(),
    };
    dt_console_server::repositories::control_log_repository::ControlLogRepository::create(
        &pool, &orphan1,
    )
    .await
    .unwrap();

    let orphan2 = dt_console_server::models::ControlLog {
        id: 0,
        task_id: task_id_xyz.clone(),
        run_id: Some("run-2".to_string()),
        action: "stop".to_string(),
        intent_or_result: "intent".to_string(),
        operator_id: Some("operator1".to_string()),
        created_at: now,
    };
    dt_console_server::repositories::control_log_repository::ControlLogRepository::create(
        &pool, &orphan2,
    )
    .await
    .unwrap();

    // Verify orphaned intents exist.
    let orphans = dt_console_server::repositories::control_log_repository::ControlLogRepository::find_orphaned_intents(&pool)
        .await
        .unwrap();
    assert_eq!(
        orphans.len(),
        2,
        "should find 2 orphaned intents before finalisation"
    );

    // Call finalisation (simulating orchestrator restart).
    dt_console_server::control_log_handlers::finalise_orphaned_intents(&pool).await;

    // Verify orphaned intents are now finalised.
    let orphans_after = dt_console_server::repositories::control_log_repository::ControlLogRepository::find_orphaned_intents(&pool)
        .await
        .unwrap();
    assert_eq!(
        orphans_after.len(),
        0,
        "should have 0 orphaned intents after finalisation"
    );

    // Verify synthetic result rows were created.
    let all_logs =
        dt_console_server::repositories::control_log_repository::ControlLogRepository::list(&pool)
            .await
            .unwrap();
    let synthetic_results: Vec<_> = all_logs
        .iter()
        .filter(|l| l.intent_or_result == "result:orphaned_by_restart")
        .collect();
    assert_eq!(
        synthetic_results.len(),
        2,
        "should have 2 synthetic result rows"
    );

    // Verify synthetic rows reference the same task_id, run_id, and operator_id.
    let start_result = synthetic_results
        .iter()
        .find(|l| l.task_id == task_id_abc && l.run_id.as_deref() == Some("run-1"))
        .unwrap();
    assert_eq!(start_result.action, "start");
    assert_eq!(start_result.operator_id.as_deref(), Some("admin"));

    let stop_result = synthetic_results
        .iter()
        .find(|l| l.task_id == task_id_xyz && l.run_id.as_deref() == Some("run-2"))
        .unwrap();
    assert_eq!(stop_result.action, "stop");
    assert_eq!(stop_result.operator_id.as_deref(), Some("operator1"));

    // Finalisation is idempotent: calling again should not create more rows.
    dt_console_server::control_log_handlers::finalise_orphaned_intents(&pool).await;
    let all_logs2 =
        dt_console_server::repositories::control_log_repository::ControlLogRepository::list(&pool)
            .await
            .unwrap();
    let synthetic2: Vec<_> = all_logs2
        .iter()
        .filter(|l| l.intent_or_result == "result:orphaned_by_restart")
        .collect();
    assert_eq!(
        synthetic2.len(),
        2,
        "idempotent: should still have exactly 2 synthetic rows"
    );
}

/// Repository unit test: ControlLog phase_and_result parsing.
#[actix_web::test]
async fn test_control_log_phase_and_result_parsing() {
    let intent_log = dt_console_server::models::ControlLog {
        id: 1,
        task_id: "t1".to_string(),
        run_id: Some("r1".to_string()),
        action: "start".to_string(),
        intent_or_result: "intent".to_string(),
        operator_id: Some("admin".to_string()),
        created_at: "2025-01-01T00:00:00.000Z".to_string(),
    };
    let (phase, result) = intent_log.phase_and_result();
    assert_eq!(phase, "intent");
    assert!(result.is_none());

    let success_log = dt_console_server::models::ControlLog {
        id: 2,
        task_id: "t1".to_string(),
        run_id: Some("r1".to_string()),
        action: "start".to_string(),
        intent_or_result: "result:success".to_string(),
        operator_id: Some("admin".to_string()),
        created_at: "2025-01-01T00:00:00.001Z".to_string(),
    };
    let (phase, result) = success_log.phase_and_result();
    assert_eq!(phase, "result");
    assert_eq!(result.unwrap(), "success");

    let orphaned_log = dt_console_server::models::ControlLog {
        id: 3,
        task_id: "t1".to_string(),
        run_id: Some("r1".to_string()),
        action: "start".to_string(),
        intent_or_result: "result:orphaned_by_restart".to_string(),
        operator_id: Some("admin".to_string()),
        created_at: "2025-01-01T00:00:00.002Z".to_string(),
    };
    let (phase, result) = orphaned_log.phase_and_result();
    assert_eq!(phase, "result");
    assert_eq!(result.unwrap(), "orphaned_by_restart");
}

/// Repository unit test: ControlLog to_response conversion.
#[actix_web::test]
async fn test_control_log_to_response() {
    let log = dt_console_server::models::ControlLog {
        id: 42,
        task_id: "task-123".to_string(),
        run_id: Some("run-456".to_string()),
        action: "stop".to_string(),
        intent_or_result: "result:success".to_string(),
        operator_id: Some("admin".to_string()),
        created_at: "2025-01-01T00:00:00.000Z".to_string(),
    };
    let resp = log.to_response();
    assert_eq!(resp.id, 42);
    assert_eq!(resp.task_id, "task-123");
    assert_eq!(resp.run_id, Some("run-456".to_string()));
    assert_eq!(resp.action, "stop");
    assert_eq!(resp.phase, "result");
    assert_eq!(resp.result, Some("success".to_string()));
    assert_eq!(resp.operator_id, Some("admin".to_string()));
    assert_eq!(resp.ts, "2025-01-01T00:00:00.000Z");
}

// ═══════════════════════════════════════════════════════════════════════════
// TOCTOU RACE TEST: concurrent start_task for the same task_id
// ═══════════════════════════════════════════════════════════════════════════

/// Two concurrent POST /api/tasks/:id/start requests for the same task
/// must be serialized by the active_runs mutex so that exactly one
/// succeeds (202) and the other gets 409 RUN_ALREADY_ACTIVE.
#[actix_web::test]
async fn test_concurrent_start_task_race_only_one_succeeds() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create a task.
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    std::env::set_var("APE_DTS_BINARY_PATH", "sleep");

    // Fire two concurrent start requests.
    let req1 = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();

    let req2 = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();

    let (resp1, resp2) = tokio::join!(
        test::call_service(&app, req1),
        test::call_service(&app, req2),
    );

    let status1 = resp1.status();
    let status2 = resp2.status();

    // Exactly one must be 202 (ACCEPTED) and the other 409 (CONFLICT).
    let accepted_count = [status1, status2]
        .iter()
        .filter(|s| **s == StatusCode::ACCEPTED)
        .count();
    let conflict_count = [status1, status2]
        .iter()
        .filter(|s| **s == StatusCode::CONFLICT)
        .count();

    assert_eq!(
        accepted_count, 1,
        "exactly one start should succeed (got statuses: {status1}, {status2})"
    );
    assert_eq!(
        conflict_count, 1,
        "exactly one start should be rejected with 409 (got statuses: {status1}, {status2})"
    );

    // The 409 response should have code RUN_ALREADY_ACTIVE.
    let conflict_resp = if status1 == StatusCode::CONFLICT {
        resp1
    } else {
        resp2
    };
    let conflict_body: serde_json::Value = test::read_body_json(conflict_resp).await;
    assert_eq!(conflict_body["code"], "RUN_ALREADY_ACTIVE");

    // Clean up: stop the running task.
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let _ = test::call_service(&app, req).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    std::env::remove_var("APE_DTS_BINARY_PATH");
    cleanup_run_dirs(&pool, &task_id).await;
}

/// When the license max_tasks cap is reached, POST /api/tasks/:id/start
/// must return 422 LICENSE_LIMIT_EXCEEDED.
#[actix_web::test]
async fn test_start_task_blocked_by_license_cap() {
    let (pool, active_runs) = setup().await;

    // Overwrite the license to allow only 1 task.
    let existing = LicenseRepository::get_current(&pool)
        .await
        .unwrap()
        .unwrap();
    let mut lic = existing;
    lic.max_tasks = 1;
    LicenseRepository::update(&pool, &lic).await.unwrap();

    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create the one allowed task.
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                "filter": { "doDbs": ["testdb"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let task_body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_body["id"].as_str().unwrap().to_string();

    // Create a second task (at cap, creation should be blocked).
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://db-src2.example.com:3307/testdb2" },
                "targetEndpoint": { "url": "mysql://db-dst2.example.com:3308/testdb2" },
                "filter": { "doDbs": ["testdb2"] }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    // Creation at cap is blocked — 422 LICENSE_LIMIT_EXCEEDED.
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "LICENSE_LIMIT_EXCEEDED");

    // Now try to start the one existing task — this should be blocked
    // because we're at 1 task / 1 max, which is at the cap.
    std::env::set_var("APE_DTS_BINARY_PATH", "sleep");
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/start")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    // At cap (1 task, max=1) — start blocked: current_tasks >= max_tasks.
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "LICENSE_LIMIT_EXCEEDED");
    assert_eq!(body["details"]["maxTasks"], 1);
    assert_eq!(body["details"]["currentTasks"], 1);

    // Restore a generous license so cleanup can proceed.
    let existing = LicenseRepository::get_current(&pool)
        .await
        .unwrap()
        .unwrap();
    let mut lic = existing;
    lic.max_tasks = 100;
    LicenseRepository::update(&pool, &lic).await.unwrap();

    // Clean up.
    std::env::remove_var("APE_DTS_BINARY_PATH");
    cleanup_run_dirs(&pool, &task_id).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// SIGNAL DELIVERY AND IDEMPOTENCY NAMESPACING
//
// These exercise the stop endpoint against Runs seeded directly into the DB,
// with no in-memory handle — the path that signals the engine by PID. That is
// deliberate: it needs no engine process, so it does not depend on precheck
// being able to reach the task's endpoints.
// ═══════════════════════════════════════════════════════════════════════════

/// Create a task through the API and evaluate to its id.
macro_rules! create_task {
    ($app:expr, $cookies:expr) => {{
        let req = add_auth(
            test::TestRequest::post()
                .uri("/api/tasks")
                .set_json(serde_json::json!({
                    "kind": "snapshot",
                    "engineSource": "mysql",
                    "engineTarget": "mysql",
                    "sourceEndpoint": { "url": "mysql://db-src.example.com:3307/testdb" },
                    "targetEndpoint": { "url": "mysql://db-dst.example.com:3308/testdb" },
                    "filter": { "doDbs": ["testdb"] }
                })),
            $cookies,
        )
        .to_request();
        let resp = test::call_service(&$app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body: serde_json::Value = test::read_body_json(resp).await;
        body["id"].as_str().unwrap().to_string()
    }};
}

/// A pid that is genuinely dead: spawn a child, kill it, reap it. A large
/// constant would be a slow-burning flake — the kernel may hand that number
/// to somebody else, and the test would signal a stranger.
fn reaped_pid() -> u32 {
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    let _ = child.kill();
    let _ = child.wait();
    pid
}

/// Seed a `running` Run for `task_id` with the given pid and log dir.
async fn seed_running_run(
    pool: &SqlitePool,
    task_id: &str,
    pid: Option<i64>,
    log_dir: Option<String>,
) -> String {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let run = dt_console_server::models::Run {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: Some(task_id.to_string()),
        status: "running".to_string(),
        pid,
        ini_path: None,
        log_dir,
        started_at: Some(now.clone()),
        stopped_at: None,
        exit_code: None,
        stop_method: None,
        metrics_port: None,
        resumed_from_run_id: None,
        created_at: now.clone(),
        updated_at: now,
    };
    RunRepository::create(pool, &run).await.unwrap();
    run.id
}

#[actix_web::test]
async fn test_stop_of_already_exited_process_succeeds() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");
    let task_id = create_task!(app, &cookies);

    // A pid that is not in use: `kill` reports ESRCH, which means the process
    // is already gone — the caller's intent already holds, so stop succeeds.
    let run_id = seed_running_run(&pool, &task_id, Some(reaped_pid() as i64), None).await;

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
    assert_eq!(run.status, "stopped");
}

#[actix_web::test]
async fn test_stop_leaves_run_running_when_signal_cannot_be_delivered() {
    // pid 1 belongs to root; an unprivileged process gets EPERM. As root every
    // signal lands, so there is no undeliverable pid to test with — and we are
    // certainly not SIGTERM-ing init to find out.
    #[cfg(unix)]
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: running as root, no undeliverable pid available");
        return;
    }

    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");
    let task_id = create_task!(app, &cookies);

    let run_id = seed_running_run(&pool, &task_id, Some(1), None).await;

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "an undeliverable signal must surface as an error"
    );

    let run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
    assert_eq!(
        run.status, "running",
        "the run must not be marked stopped while its process is still alive"
    );
}

// ─── Idempotency-Key namespacing ────────────────────────────────────────
//
// The cache key is `user_id:method:path:key`. With a bare key, one key reused
// across two endpoints replayed the first call's cached 202 for the second —
// the second action never ran, while the API reported success.

#[actix_web::test]
async fn test_same_idempotency_key_on_a_different_task_is_not_replayed() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let task_a = create_task!(app, &cookies);
    let task_b = create_task!(app, &cookies);
    seed_running_run(&pool, &task_a, Some(reaped_pid() as i64), None).await;
    // Task B has no active Run at all.

    let key = "one-key-for-everything";

    let req = add_auth(
        test::TestRequest::post()
            .uri(&format!("/api/tasks/{task_a}/stop"))
            .insert_header(("Idempotency-Key", key)),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let req = add_auth(
        test::TestRequest::post()
            .uri(&format!("/api/tasks/{task_b}/stop"))
            .insert_header(("Idempotency-Key", key)),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "task B must be judged on its own state, not replay task A's 202"
    );
}

#[actix_web::test]
async fn test_replayed_idempotency_key_on_the_same_endpoint_is_still_cached() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");

    let task_id = create_task!(app, &cookies);
    seed_running_run(&pool, &task_id, Some(reaped_pid() as i64), None).await;

    let key = "retry-the-same-stop";
    let mut statuses = Vec::new();
    for _ in 0..2 {
        let req = add_auth(
            test::TestRequest::post()
                .uri(&format!("/api/tasks/{task_id}/stop"))
                .insert_header(("Idempotency-Key", key)),
            &cookies,
        )
        .to_request();
        statuses.push(test::call_service(&app, req).await.status());
    }

    // Without the cache the second call would be a 409: the run is no longer
    // active. Dedup within the namespace still has to work.
    assert_eq!(statuses[0], StatusCode::ACCEPTED);
    assert_eq!(
        statuses[1],
        StatusCode::ACCEPTED,
        "a retried stop with the same key must replay the cached 202"
    );
}

// ─── Log file whitelist ─────────────────────────────────────────────────

#[actix_web::test]
async fn test_log_endpoint_serves_child_stdout_and_stderr() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");
    let task_id = create_task!(app, &cookies);

    // Stand in for the child-output capture the executor sets up: when the
    // engine dies before log4rs is up, this is the only record of why.
    let log_dir = std::env::temp_dir().join(format!("dt-log-whitelist-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&log_dir).unwrap();
    std::fs::write(log_dir.join("stdout.log"), "engine says hello\n").unwrap();
    std::fs::write(
        log_dir.join("stderr.log"),
        "init failed: cannot connect to source\n",
    )
    .unwrap();

    let run_id = seed_running_run(
        &pool,
        &task_id,
        Some(reaped_pid() as i64),
        Some(log_dir.to_string_lossy().to_string()),
    )
    .await;

    for (file, expected) in [
        ("stdout", "engine says hello"),
        ("stderr", "init failed: cannot connect to source"),
    ] {
        let req = add_auth(
            test::TestRequest::get().uri(&format!("/api/runs/{run_id}/logs?file={file}")),
            &cookies,
        )
        .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "{file} must be a readable log file"
        );
        let body = test::read_body(resp).await;
        assert!(
            String::from_utf8_lossy(&body).contains(expected),
            "{file} content should be served verbatim"
        );
    }

    // The whitelist still holds against everything else.
    let req = add_auth(
        test::TestRequest::get().uri(&format!("/api/runs/{run_id}/logs?file=passwd")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_ne!(resp.status(), StatusCode::OK, "unknown files stay rejected");

    let _ = std::fs::remove_dir_all(&log_dir);
}

#[actix_web::test]
async fn test_pause_of_a_dead_engine_is_refused() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");
    let task_id = create_task!(app, &cookies);

    let run_id = seed_running_run(&pool, &task_id, Some(reaped_pid() as i64), None).await;

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/pause")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "there is no engine left to pause"
    );

    let run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
    assert_eq!(
        run.status, "running",
        "the run must not be recorded as paused when nothing was paused"
    );
}

/// VAL-CTRL-003: pause writes intent + result, and so does discarding the
/// paused Run with a stop.
///
/// Deliberately seeds the Run rows instead of calling `start`: `start` runs
/// precheck against the fake endpoints these tests use and is refused with
/// 422, which would make this a test of precheck rather than of control logs.
#[actix_web::test]
async fn test_control_log_pause_and_discard_each_write_intent_and_result() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");
    let task_id = create_task!(app, &cookies);

    // A live child so the pause SIGTERM has a real process to reach.
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn sleep");
    let run_id = seed_running_run(&pool, &task_id, Some(child.id() as i64), None).await;

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/pause")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let _ = child.wait();

    let logs =
        dt_console_server::repositories::control_log_repository::ControlLogRepository::list(&pool)
            .await
            .unwrap();
    let pause_logs: Vec<_> = logs
        .iter()
        .filter(|l| l.action == "pause" && l.run_id.as_deref() == Some(&run_id))
        .collect();
    assert_eq!(
        pause_logs.len(),
        2,
        "should have exactly 2 pause logs (intent + result)"
    );
    assert!(pause_logs.iter().any(|l| l.intent_or_result == "intent"));
    assert!(pause_logs
        .iter()
        .any(|l| l.intent_or_result.starts_with("result:")));

    // There is no supervisor in this test app, so land the Run in `paused`
    // the way the supervisor would, then discard it.
    let mut run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
    assert_eq!(
        run.status, "pausing",
        "pause must not write `paused` itself"
    );
    run.status = "paused".to_string();
    RunRepository::update(&pool, &run).await.unwrap();

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let logs =
        dt_console_server::repositories::control_log_repository::ControlLogRepository::list(&pool)
            .await
            .unwrap();
    let stop_logs: Vec<_> = logs
        .iter()
        .filter(|l| l.action == "stop" && l.run_id.as_deref() == Some(&run_id))
        .collect();
    assert_eq!(stop_logs.len(), 2, "stop writes intent + result too");
}

/// Pause is a SIGTERM with intent: the Run goes to `pausing` (never straight
/// to `paused`) and the engine process is actually signalled.
#[actix_web::test]
async fn test_pause_marks_pausing_and_signals_the_engine() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");
    let task_id = create_task!(app, &cookies);

    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    let run_id = seed_running_run(&pool, &task_id, Some(pid as i64), None).await;

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/pause")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
    assert_eq!(
        run.status, "pausing",
        "only the supervisor, seeing the exit code, may write `paused`"
    );

    // The child got a real SIGTERM.
    let status = child.wait().expect("child should have been signalled");
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(libc::SIGTERM));
    }
    #[cfg(not(unix))]
    let _ = status;
}

/// Pause is refused for kinds with no resumable position.
#[actix_web::test]
async fn test_pause_rejected_for_check_kind() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");
    let task_id = create_task!(app, &cookies);

    // `kind` is immutable through the API, so rewrite it directly.
    sqlx::query("UPDATE tasks SET kind = 'check' WHERE id = ?")
        .bind(&task_id)
        .execute(&pool)
        .await
        .unwrap();

    let dead = reaped_pid();
    seed_running_run(&pool, &task_id, Some(dead as i64), None).await;

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/pause")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "UNSUPPORTED_FOR_KIND");
    assert_eq!(body["details"]["kind"], "check");
}

/// Stopping a `paused` Run discards its position instead of signalling a
/// process that pause already ended.
#[actix_web::test]
async fn test_stop_of_paused_run_is_discarded_without_a_signal() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");
    let task_id = create_task!(app, &cookies);

    // A live child standing in for a *recycled* pid: a paused Run's pid
    // column still holds the number of the process pause ended, and stop
    // must not fire a signal at whoever inherited it.
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    let run_id = seed_running_run(&pool, &task_id, Some(pid as i64), None).await;
    let mut run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
    run.status = "paused".to_string();
    RunRepository::update(&pool, &run).await.unwrap();

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
    assert_eq!(run.status, "stopped");
    assert_eq!(run.stop_method.as_deref(), Some("discarded"));

    assert!(
        dt_console_server::signal::is_alive(pid),
        "stopping a paused Run must not signal the pid it used to own"
    );
    let _ = child.kill();
    let _ = child.wait();
}

/// A resume needs the paused Run's position log; without one it would
/// silently restart the task from its original start marker.
#[actix_web::test]
async fn test_resume_rejected_when_the_paused_run_has_no_position_log() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");
    let task_id = create_task!(app, &cookies);

    let run_id = seed_running_run(&pool, &task_id, Some(reaped_pid() as i64), None).await;
    let mut run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
    run.status = "paused".to_string();
    run.log_dir = None;
    RunRepository::update(&pool, &run).await.unwrap();

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/resume")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "ILLEGAL_TRANSITION");
}

/// A `log_dir` column is not a position log: the directory may have been
/// cleaned up, and the engine only *warns* about a missing recovery file
/// before starting from the task's original marker — a silent re-migration.
#[actix_web::test]
async fn test_resume_rejected_when_the_position_log_file_is_missing() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");
    let task_id = create_task!(app, &cookies);

    // A log dir that exists but holds no position.log.
    let log_dir = std::env::temp_dir().join(format!("dt-resume-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&log_dir).unwrap();

    let run_id = seed_running_run(
        &pool,
        &task_id,
        Some(reaped_pid() as i64),
        Some(log_dir.to_string_lossy().to_string()),
    )
    .await;
    let mut run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
    run.status = "paused".to_string();
    RunRepository::update(&pool, &run).await.unwrap();

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/resume")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "ILLEGAL_TRANSITION");

    let _ = std::fs::remove_dir_all(&log_dir);
}

/// A pause that will not converge must still be stoppable, or `is_active`
/// freezes the task with nothing able to move it.
#[actix_web::test]
async fn test_stop_can_overtake_a_pausing_run() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");
    let task_id = create_task!(app, &cookies);

    // An engine that ignores SIGTERM — exactly the case with no way out.
    let mut child = std::process::Command::new("sh")
        .args(["-c", "trap '' TERM; sleep 30"])
        .spawn()
        .expect("spawn sh");
    let pid = child.id();
    let run_id = seed_running_run(&pool, &task_id, Some(pid as i64), None).await;
    let mut run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
    run.status = "pausing".to_string();
    RunRepository::update(&pool, &run).await.unwrap();

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/stop")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
    assert_eq!(run.status, "stopped");

    // Reap before asking whether it is alive: an unreaped zombie still
    // answers `kill(pid, 0)`.
    let status = child
        .wait()
        .expect("the wedged engine must have been killed");
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            status.signal(),
            Some(libc::SIGKILL),
            "an engine that ignores SIGTERM has to be escalated"
        );
    }
    #[cfg(not(unix))]
    let _ = status;
    assert!(!dt_console_server::signal::is_alive(pid));
}

/// Resuming is only legal from the task's *latest* Run: an older paused Run
/// would rewind past everything that ran after it.
#[actix_web::test]
async fn test_resume_rejected_when_the_latest_run_is_not_paused() {
    let (pool, active_runs) = setup().await;
    let app = test::init_service(build_test_app(pool.clone(), active_runs)).await;
    let cookies = do_login!(app, "admin", "admin123");
    let task_id = create_task!(app, &cookies);

    let paused_id = seed_running_run(&pool, &task_id, Some(reaped_pid() as i64), None).await;
    let mut paused = RunRepository::find_by_id(&pool, &paused_id).await.unwrap();
    paused.status = "paused".to_string();
    RunRepository::update(&pool, &paused).await.unwrap();

    // A later Run that ended normally.
    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    let later_id = seed_running_run(&pool, &task_id, Some(reaped_pid() as i64), None).await;
    let mut later = RunRepository::find_by_id(&pool, &later_id).await.unwrap();
    later.status = "stopped".to_string();
    RunRepository::update(&pool, &later).await.unwrap();

    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/resume")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "ILLEGAL_TRANSITION");
    assert_eq!(body["details"]["from"], "stopped");
}
