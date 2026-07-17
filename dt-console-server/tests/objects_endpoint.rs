//! Integration tests for GET /api/runs/:id/objects.
//!
//! VAL-ORCH-023: 404 for unknown run id.
//! VAL-ORCH-024: 200 + [] when finished.log is missing.
//! VAL-ORCH-025: All planned tables start in pending state.
//! VAL-ORCH-026: Tables transition to completed after RdbSnapshotFinished.
//! VAL-ORCH-027: State enum is restricted to pending | loading | completed.
//! VAL-ORCH-032: Malformed finished.log line does not crash /objects.
//! VAL-ORCH-033: Duplicate RdbSnapshotFinished lines are idempotent.
//! Lines for tables NOT in planned list are ignored.

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
use dt_console_server::models::{LoginRequest, ResourceGroup, Run};
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use dt_console_server::repositories::license_repository::LicenseRepository;
use dt_console_server::repositories::resource_group_repository::ResourceGroupRepository;
use dt_console_server::repositories::run_repository::RunRepository;
use dt_console_server::run_handlers;
use sqlx::SqlitePool;

// ─── Test infrastructure ────────────────────────────────────────────────────

async fn test_pool() -> SqlitePool {
    let dir = std::env::temp_dir().join("dt-console-server-objects-endpoint-test");
    std::fs::create_dir_all(&dir).unwrap();
    let test_name = std::thread::current()
        .name()
        .unwrap_or("unknown")
        .to_string();
    let safe_name: String = test_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let path = dir.join(format!("obj-{safe_name}.db"));
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

/// Seed a task with a specific filter_config (for planned table list).
async fn seed_task_with_filter(pool: &SqlitePool, task_id: &str, filter_config: &str) {
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
    .bind(filter_config)
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

/// Seed a run with a specific log_dir pointing to a temp directory.
async fn seed_run_with_log_dir(pool: &SqlitePool, run_id: &str, task_id: &str, log_dir: &str) {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let run = Run {
        id: run_id.to_string(),
        task_id: Some(task_id.to_string()),
        status: "running".to_string(),
        pid: Some(1234),
        ini_path: None,
        log_dir: Some(log_dir.to_string()),
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

/// Write a finished.log file with the given lines.
fn write_finished_log(log_dir: &str, lines: &[&str]) {
    let log_dir_path = std::path::Path::new(log_dir);
    std::fs::create_dir_all(log_dir_path).unwrap();
    let finished_path = log_dir_path.join("finished.log");
    let content = lines.join("\n");
    std::fs::write(&finished_path, content).unwrap();
}

/// Append a line to finished.log.
fn append_finished_log(log_dir: &str, line: &str) {
    let log_dir_path = std::path::Path::new(log_dir);
    std::fs::create_dir_all(log_dir_path).unwrap();
    let finished_path = log_dir_path.join("finished.log");
    let existing = std::fs::read_to_string(&finished_path).unwrap_or_default();
    let new_content = if existing.is_empty() {
        line.to_string()
    } else {
        format!("{existing}\n{line}")
    };
    std::fs::write(&finished_path, new_content).unwrap();
}

/// Create a temp log dir for a test run.
fn make_log_dir(run_id: &str) -> String {
    let dir = std::env::temp_dir().join(format!("dt-objects-test-{run_id}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.to_string_lossy().to_string()
}

fn make_finished_line(schema: &str, table: &str) -> String {
    format!(
        r#"2024-04-01 03:25:18.701725 | {{"type":"RdbSnapshotFinished","db_type":"mysql","schema":"{schema}","tb":"{table}"}}"#
    )
}

// ─── Tests ──────────────────────────────────────────────────────────────────

/// VAL-ORCH-023: unknown run id returns 404 with RUN_NOT_FOUND envelope.
#[actix_web::test]
async fn objects_unknown_run_returns_404() {
    let pool = test_pool().await;
    setup(&pool).await;
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/nonexistent-run-id/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], "RUN_NOT_FOUND");
}

/// VAL-ORCH-024: run exists but no finished.log → 200 with literal [].
#[actix_web::test]
async fn objects_no_log_returns_empty_array() {
    let pool = test_pool().await;
    setup(&pool).await;
    let log_dir = make_log_dir("run-no-log");
    seed_task_with_filter(
        &pool,
        "task-no-log",
        r#"{"do_dbs":"","do_tbs":"test_db.t1,test_db.t2","ignore_dbs":"","ignore_tbs":""}"#,
    )
    .await;
    seed_run_with_log_dir(&pool, "run-no-log", "task-no-log", &log_dir).await;
    // Create log_dir but do NOT create finished.log at all.
    // The log_dir exists, but finished.log is absent.

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-no-log/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body, serde_json::Value::Array(vec![]));
}

/// VAL-ORCH-025: All planned tables start in pending state (no finished.log entries).
#[actix_web::test]
async fn objects_all_planned_tables_start_pending() {
    let pool = test_pool().await;
    setup(&pool).await;
    let log_dir = make_log_dir("run-pending");
    seed_task_with_filter(
        &pool,
        "task-pending",
        r#"{"do_dbs":"","do_tbs":"test_db.t1,test_db.t2","ignore_dbs":"","ignore_tbs":""}"#,
    )
    .await;
    seed_run_with_log_dir(&pool, "run-pending", "task-pending", &log_dir).await;
    // Write an empty finished.log
    write_finished_log(&log_dir, &[""]);

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-pending/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 2);

    // All entries should have state "pending"
    for entry in arr {
        assert_eq!(entry["state"], "pending");
    }

    // Check that the exact (schema, table) pairs match the planned list
    let pairs: Vec<(String, String)> = arr
        .iter()
        .map(|e| {
            (
                e["schema"].as_str().unwrap().to_string(),
                e["table"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(pairs.contains(&("test_db".to_string(), "t1".to_string())));
    assert!(pairs.contains(&("test_db".to_string(), "t2".to_string())));
}

/// VAL-ORCH-026: Tables transition to completed after their RdbSnapshotFinished event.
#[actix_web::test]
async fn objects_table_transitions_to_completed() {
    let pool = test_pool().await;
    setup(&pool).await;
    let log_dir = make_log_dir("run-completed");
    seed_task_with_filter(
        &pool,
        "task-completed",
        r#"{"do_dbs":"","do_tbs":"test_db.t1,test_db.t2","ignore_dbs":"","ignore_tbs":""}"#,
    )
    .await;
    seed_run_with_log_dir(&pool, "run-completed", "task-completed", &log_dir).await;
    // Write a finished.log with one completed table
    write_finished_log(&log_dir, &[&make_finished_line("test_db", "t1")]);

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-completed/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 2);

    // Find t1 and t2 entries
    let t1 = arr.iter().find(|e| e["table"] == "t1").unwrap();
    let t2 = arr.iter().find(|e| e["table"] == "t2").unwrap();

    assert_eq!(t1["state"], "completed");
    assert_eq!(t2["state"], "pending");
}

/// Two-table sequence: S1.T1 then S2.T2 verified independently.
#[actix_web::test]
async fn objects_two_table_sequence() {
    let pool = test_pool().await;
    setup(&pool).await;
    let log_dir = make_log_dir("run-seq");
    seed_task_with_filter(
        &pool,
        "task-seq",
        r#"{"do_dbs":"","do_tbs":"s1.t1,s2.t2","ignore_dbs":"","ignore_tbs":""}"#,
    )
    .await;
    seed_run_with_log_dir(&pool, "run-seq", "task-seq", &log_dir).await;

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    // Initially all pending
    write_finished_log(&log_dir, &[]);
    let req = auth_get("/api/runs/run-seq/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body.as_array().unwrap().len(), 2);
    for entry in body.as_array().unwrap() {
        assert_eq!(entry["state"], "pending");
    }

    // Complete s1.t1
    write_finished_log(&log_dir, &[&make_finished_line("s1", "t1")]);
    let req = auth_get("/api/runs/run-seq/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(res).await;
    let t1 = body
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["schema"] == "s1" && e["table"] == "t1")
        .unwrap();
    let t2 = body
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["schema"] == "s2" && e["table"] == "t2")
        .unwrap();
    assert_eq!(t1["state"], "completed");
    assert_eq!(t2["state"], "pending");

    // Complete s2.t2 as well
    append_finished_log(&log_dir, &make_finished_line("s2", "t2"));
    let req = auth_get("/api/runs/run-seq/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(res).await;
    for entry in body.as_array().unwrap() {
        assert_eq!(entry["state"], "completed");
    }
}

/// VAL-ORCH-027: State enum is restricted to pending | loading | completed.
#[actix_web::test]
async fn objects_states_are_valid() {
    let pool = test_pool().await;
    setup(&pool).await;
    let log_dir = make_log_dir("run-states");
    seed_task_with_filter(
        &pool,
        "task-states",
        r#"{"do_dbs":"","do_tbs":"test_db.t1,test_db.t2","ignore_dbs":"","ignore_tbs":""}"#,
    )
    .await;
    seed_run_with_log_dir(&pool, "run-states", "task-states", &log_dir).await;
    write_finished_log(&log_dir, &[&make_finished_line("test_db", "t1")]);

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-states/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(res).await;

    let valid_states = ["pending", "loading", "completed"];
    for entry in body.as_array().unwrap() {
        let state = entry["state"].as_str().unwrap();
        assert!(valid_states.contains(&state), "invalid state: {state}");
    }
}

/// VAL-ORCH-032: Malformed finished.log line does not crash /objects.
#[actix_web::test]
async fn objects_malformed_line_skipped() {
    let pool = test_pool().await;
    setup(&pool).await;
    let log_dir = make_log_dir("run-malformed");
    seed_task_with_filter(
        &pool,
        "task-malformed",
        r#"{"do_dbs":"","do_tbs":"test_db.t1","ignore_dbs":"","ignore_tbs":""}"#,
    )
    .await;
    seed_run_with_log_dir(&pool, "run-malformed", "task-malformed", &log_dir).await;
    // Write a log with a valid line, a truncated line, and another valid line
    write_finished_log(
        &log_dir,
        &[
            &make_finished_line("test_db", "t1"),
            "this is not valid json at all",
            "truncated line without braces",
        ],
    );

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-malformed/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    // Must not 5xx
    assert!(res.status().is_success());
    let body: serde_json::Value = test::read_body_json(res).await;
    let arr = body.as_array().unwrap();
    // t1 should be completed
    let t1 = arr.iter().find(|e| e["table"] == "t1").unwrap();
    assert_eq!(t1["state"], "completed");
}

/// VAL-ORCH-033: Duplicate RdbSnapshotFinished lines are idempotent.
#[actix_web::test]
async fn objects_duplicate_lines_idempotent() {
    let pool = test_pool().await;
    setup(&pool).await;
    let log_dir = make_log_dir("run-dup");
    seed_task_with_filter(
        &pool,
        "task-dup",
        r#"{"do_dbs":"","do_tbs":"test_db.t1","ignore_dbs":"","ignore_tbs":""}"#,
    )
    .await;
    seed_run_with_log_dir(&pool, "run-dup", "task-dup", &log_dir).await;
    // Write two identical RdbSnapshotFinished lines
    write_finished_log(
        &log_dir,
        &[
            &make_finished_line("test_db", "t1"),
            &make_finished_line("test_db", "t1"),
        ],
    );

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-dup/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    let arr = body.as_array().unwrap();
    // Exactly one row for (test_db, t1), not two
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["schema"], "test_db");
    assert_eq!(arr[0]["table"], "t1");
    assert_eq!(arr[0]["state"], "completed");
}

/// Lines for tables NOT in planned list are ignored.
#[actix_web::test]
async fn objects_ignores_unplanned_tables() {
    let pool = test_pool().await;
    setup(&pool).await;
    let log_dir = make_log_dir("run-unplanned");
    seed_task_with_filter(
        &pool,
        "task-unplanned",
        r#"{"do_dbs":"","do_tbs":"test_db.t1","ignore_dbs":"","ignore_tbs":""}"#,
    )
    .await;
    seed_run_with_log_dir(&pool, "run-unplanned", "task-unplanned", &log_dir).await;
    // Write finished.log with a table that's NOT in the planned list
    write_finished_log(
        &log_dir,
        &[
            &make_finished_line("test_db", "t1"),
            &make_finished_line("other_db", "unplanned_table"),
        ],
    );

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-unplanned/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    let arr = body.as_array().unwrap();
    // Only t1 in response, not the unplanned table
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["schema"], "test_db");
    assert_eq!(arr[0]["table"], "t1");
    assert_eq!(arr[0]["state"], "completed");
}

/// Run with NULL log_dir returns 200 + [] (no log dir to read).
#[actix_web::test]
async fn objects_null_log_dir_returns_empty() {
    let pool = test_pool().await;
    setup(&pool).await;
    seed_task_with_filter(
        &pool,
        "task-null-log",
        r#"{"do_dbs":"","do_tbs":"test_db.t1","ignore_dbs":"","ignore_tbs":""}"#,
    )
    .await;
    // Seed a run with log_dir = None
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let run = Run {
        id: "run-null-log".to_string(),
        task_id: Some("task-null-log".to_string()),
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
    RunRepository::create(&pool, &run).await.unwrap();

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-null-log/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body, serde_json::Value::Array(vec![]));
}

/// Run with task_id = None (orphaned) returns 200 + [].
#[actix_web::test]
async fn objects_orphaned_run_no_task_returns_empty() {
    let pool = test_pool().await;
    setup(&pool).await;
    let log_dir = make_log_dir("run-orphan");
    // Seed a run with no task_id
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let run = Run {
        id: "run-orphan".to_string(),
        task_id: None,
        status: "running".to_string(),
        pid: Some(1234),
        ini_path: None,
        log_dir: Some(log_dir),
        started_at: Some(now.clone()),
        stopped_at: None,
        exit_code: None,
        stop_method: None,
        metrics_port: None,
        created_at: now.clone(),
        updated_at: now,
    };
    RunRepository::create(&pool, &run).await.unwrap();

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-orphan/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body, serde_json::Value::Array(vec![]));
}

/// Filter with array-style do_tbs works the same as comma-separated.
#[actix_web::test]
async fn objects_array_do_tbs() {
    let pool = test_pool().await;
    setup(&pool).await;
    let log_dir = make_log_dir("run-arr");
    seed_task_with_filter(
        &pool,
        "task-arr",
        r#"{"do_dbs":[],"do_tbs":["test_db.t1","test_db.t2"],"ignore_dbs":[],"ignore_tbs":[]}"#,
    )
    .await;
    seed_run_with_log_dir(&pool, "run-arr", "task-arr", &log_dir).await;
    write_finished_log(&log_dir, &[]);

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-arr/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    for entry in arr {
        assert_eq!(entry["state"], "pending");
    }
}

/// Wildcard do_tbs patterns include completed tables that match the wildcard.
#[actix_web::test]
async fn objects_wildcard_includes_completed() {
    let pool = test_pool().await;
    setup(&pool).await;
    let log_dir = make_log_dir("run-wildcard");
    seed_task_with_filter(
        &pool,
        "task-wildcard",
        r#"{"do_dbs":"*","do_tbs":"*.*","ignore_dbs":"","ignore_tbs":""}"#,
    )
    .await;
    seed_run_with_log_dir(&pool, "run-wildcard", "task-wildcard", &log_dir).await;
    write_finished_log(&log_dir, &[&make_finished_line("test_db", "manual_smoke")]);

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-wildcard/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    let arr = body.as_array().unwrap();
    // The *.* wildcard matches all tables, so the completed table should appear.
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["schema"], "test_db");
    assert_eq!(arr[0]["table"], "manual_smoke");
    assert_eq!(arr[0]["state"], "completed");
}

/// Schema wildcard do_tbs includes completed tables in that schema.
#[actix_web::test]
async fn objects_schema_wildcard_includes_completed() {
    let pool = test_pool().await;
    setup(&pool).await;
    let log_dir = make_log_dir("run-sw");
    seed_task_with_filter(
        &pool,
        "task-sw",
        r#"{"do_dbs":"test_db","do_tbs":"test_db.*","ignore_dbs":"","ignore_tbs":""}"#,
    )
    .await;
    seed_run_with_log_dir(&pool, "run-sw", "task-sw", &log_dir).await;
    // Write completed entry for a table in test_db, and one in another schema (should be ignored).
    write_finished_log(
        &log_dir,
        &[
            &make_finished_line("test_db", "table1"),
            &make_finished_line("other_db", "table2"),
        ],
    );

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app);

    let req = auth_get("/api/runs/run-sw/objects", &cookies).to_request();
    let res = test::call_service(&app, req).await;

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    let arr = body.as_array().unwrap();
    // Only test_db.table1 should be included (matches schema wildcard).
    // other_db.table2 doesn't match test_db.* so it's ignored.
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["schema"], "test_db");
    assert_eq!(arr[0]["table"], "table1");
    assert_eq!(arr[0]["state"], "completed");
}
