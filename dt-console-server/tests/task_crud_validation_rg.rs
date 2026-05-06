//! Integration tests for Task CRUD + Resource Group CRUD + validation + security.

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
use dt_console_server::middleware::csrf::{Csrf, XSRF_COOKIE_NAME, XSRF_HEADER_NAME};
use dt_console_server::models::{LoginRequest, ResourceGroup};
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use dt_console_server::repositories::resource_group_repository::ResourceGroupRepository;
use dt_console_server::repositories::run_repository::RunRepository;
use sqlx::SqlitePool;

// ─── Helpers ─────────────────────────────────────────────────────────────

async fn test_pool() -> SqlitePool {
    let dir = std::env::temp_dir().join("dt-console-server-task-test");
    std::fs::create_dir_all(&dir).unwrap();
    let test_name = std::thread::current()
        .name()
        .unwrap_or("unknown")
        .to_string();
    let safe_name: String = test_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let path = dir.join(format!("task-{safe_name}.db"));
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

fn add_cookies(mut req: test::TestRequest, cookies: &[Cookie<'static>]) -> test::TestRequest {
    for c in cookies {
        req = req.cookie(c.clone());
    }
    req
}

fn add_auth(mut req: test::TestRequest, cookies: &[Cookie<'static>]) -> test::TestRequest {
    req = add_cookies(req, cookies);
    // Override the server-generated XSRF cookie so it matches the header value
    req = req.cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF));
    req = req.insert_header((XSRF_HEADER_NAME, XSRF));
    req
}

// Task body helpers
fn snapshot_task_body() -> serde_json::Value {
    serde_json::json!({
        "kind": "snapshot",
        "engineSource": "mysql",
        "engineTarget": "mysql",
        "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/src_db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/dst_db"},
        "extractor": {},
        "sinker": {},
        "filter": {},
        "router": {},
        "parallelizer": {"parallelSize": 2},
        "pipeline": {"bufferSize": 4},
        "resumer": {},
        "processor": {},
        "runtime": {},
        "metrics": {}
    })
}

fn cdc_task_body() -> serde_json::Value {
    serde_json::json!({
        "kind": "cdc",
        "engineSource": "mysql",
        "engineTarget": "mysql",
        "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/src_db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/dst_db"},
        "extractor": {"server_id": "1"},
        "sinker": {},
        "filter": {},
        "router": {},
        "parallelizer": {},
        "pipeline": {},
        "resumer": {},
        "processor": {},
        "runtime": {},
        "metrics": {}
    })
}

fn check_task_body() -> serde_json::Value {
    serde_json::json!({
        "kind": "check",
        "engineSource": "mysql",
        "engineTarget": "mysql",
        "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/src_db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/dst_db"},
        "extractor": {},
        "sinker": {"check_log_dir": "./check"},
        "filter": {},
        "router": {},
        "parallelizer": {},
        "pipeline": {},
        "resumer": {},
        "processor": {},
        "runtime": {},
        "metrics": {}
    })
}

fn struct_task_body() -> serde_json::Value {
    serde_json::json!({
        "kind": "struct",
        "engineSource": "mysql",
        "engineTarget": "mysql",
        "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/src_db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/dst_db"},
        "extractor": {"extract_type": "struct"},
        "sinker": {},
        "filter": {"do_dbs": ["mydb"]},
        "router": {},
        "parallelizer": {},
        "pipeline": {},
        "resumer": {},
        "processor": {},
        "runtime": {},
        "metrics": {}
    })
}

async fn seed_default_rg(pool: &SqlitePool) {
    let existing = ResourceGroupRepository::list(pool).await.unwrap();
    if existing.is_empty() {
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
}

async fn activate_license(pool: &SqlitePool, max_tasks: i64) {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let license = dt_console_server::models::License {
        id: uuid::Uuid::new_v4().to_string(),
        sku: "pro".to_string(),
        max_tasks,
        expire_at: Some("2099-12-31T23:59:59Z".to_string()),
        activated_at: Some(now.clone()),
        activation_code_hash: Some("test".to_string()),
        granted_to: "test".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };
    dt_console_server::repositories::license_repository::LicenseRepository::create(pool, &license)
        .await
        .unwrap();
}

/// Setup helper: creates pool, seeds admin + default RG + license.
async fn setup() -> SqlitePool {
    let pool = test_pool().await;
    auth::seed_admin(&pool).await.unwrap();
    seed_default_rg(&pool).await;
    activate_license(&pool, 100).await;
    pool
}

// ═══════════════════════════════════════════════════════════════════════════
// TASK CRUD TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[actix_web::test]
async fn create_task_snapshot_201() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(snapshot_task_body()),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["kind"], "snapshot");
    assert_eq!(body["status"], "draft");
}

#[actix_web::test]
async fn create_task_cdc_201() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(cdc_task_body()),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["kind"], "cdc");
}

#[actix_web::test]
async fn create_task_check_201() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(check_task_body()),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["kind"], "check");
}

#[actix_web::test]
async fn create_task_struct_201() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(struct_task_body()),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["kind"], "struct");
}

#[actix_web::test]
async fn get_task_200() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(snapshot_task_body()),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    let id = body["id"].as_str().unwrap().to_string();

    // Get
    let req = add_cookies(
        test::TestRequest::get().uri(&format!("/api/tasks/{id}")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["id"], id);
}

#[actix_web::test]
async fn get_task_not_found_404() {
    let pool = test_pool().await;
    auth::seed_admin(&pool).await.unwrap();
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_cookies(
        test::TestRequest::get().uri("/api/tasks/00000000-0000-0000-0000-000000000000"),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "task_not_found");
}

#[actix_web::test]
async fn update_task_mutable_fields_200() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(snapshot_task_body()),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    let id = body["id"].as_str().unwrap().to_string();

    // PATCH
    let req = add_auth(
        test::TestRequest::patch()
            .uri(&format!("/api/tasks/{id}"))
            .set_json(serde_json::json!({
                "filter": {"do_tbs": ["db1.tbl_*"]},
                "parallelizer": {"parallelSize": 8}
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["filter"]["do_tbs"][0], "db1.tbl_*");
}

#[actix_web::test]
async fn update_task_kind_immutable_422() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create snapshot task
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(snapshot_task_body()),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    let id = body["id"].as_str().unwrap().to_string();

    // PATCH: change kind
    let req = add_auth(
        test::TestRequest::patch()
            .uri(&format!("/api/tasks/{id}"))
            .set_json(serde_json::json!({"kind": "cdc"})),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "task_kind_immutable");
}

#[actix_web::test]
async fn delete_task_no_run_204() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(snapshot_task_body()),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    let id = body["id"].as_str().unwrap().to_string();

    // Delete
    let req = add_auth(
        test::TestRequest::delete().uri(&format!("/api/tasks/{id}")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify gone
    let req = add_cookies(
        test::TestRequest::get().uri(&format!("/api/tasks/{id}")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn delete_task_active_run_409() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create task
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(snapshot_task_body()),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    let task_id = body["id"].as_str().unwrap().to_string();

    // Insert active run
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let run = dt_console_server::models::Run {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: task_id.clone(),
        status: "running".to_string(),
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
    RunRepository::create(&pool, &run).await.unwrap();

    // Delete blocked
    let req = add_auth(
        test::TestRequest::delete().uri(&format!("/api/tasks/{task_id}")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "task_has_active_run");
}

#[actix_web::test]
async fn list_tasks_basic() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_cookies(test::TestRequest::get().uri("/api/tasks"), &cookies).to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body["items"].is_array());
    assert!(body["total"].is_number());
}

#[actix_web::test]
async fn list_tasks_filter_category() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create snapshot task
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(snapshot_task_body()),
        &cookies,
    )
    .to_request();
    test::call_service(&app, req).await;

    // Filter by category=snapshot
    let req = add_cookies(
        test::TestRequest::get().uri("/api/tasks?category=snapshot"),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert!(!items.is_empty());
    for item in items {
        assert_eq!(item["kind"], "snapshot");
    }

    // Filter by category=cdc → empty
    let req = add_cookies(
        test::TestRequest::get().uri("/api/tasks?category=cdc"),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total"], 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// GAUSSDB VALIDATION
// ═══════════════════════════════════════════════════════════════════════════

#[actix_web::test]
async fn gaussdb_without_sub_mode_422() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let body = serde_json::json!({
        "kind": "snapshot",
        "engineSource": "gaussdb",
        "engineTarget": "mysql",
        "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/db"},
    });

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "gaussdb_sub_mode_required");
}

#[actix_web::test]
async fn gaussdb_pg_mode_201() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let body = serde_json::json!({
        "kind": "snapshot",
        "engineSource": "gaussdb",
        "engineTarget": "mysql",
        "subMode": "pg-mode",
        "sourceEndpoint": {"url": "postgres://203.0.113.1:5432/db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/db"},
    });

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["dbTypeSource"], "gaussdb_pg");
}

#[actix_web::test]
async fn gaussdb_mysql_mode_201() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let body = serde_json::json!({
        "kind": "cdc",
        "engineSource": "gaussdb",
        "engineTarget": "mysql",
        "subMode": "mysql-mode",
        "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/db"},
        "extractor": {"server_id": "1"},
    });

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["dbTypeSource"], "gaussdb_mysql");
}

#[actix_web::test]
async fn gaussdb_oracle_mode_201() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let body = serde_json::json!({
        "kind": "snapshot",
        "engineSource": "gaussdb",
        "engineTarget": "oracle",
        "subMode": "oracle-mode",
        "sourceEndpoint": {"url": "oracle://203.0.113.1:1521/db"},
        "targetEndpoint": {"url": "oracle://203.0.113.2:1521/db"},
    });

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["dbTypeSource"], "gaussdb_oracle");
}

#[actix_web::test]
async fn snapshot_rejects_cdc_extract_type_422() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let body = serde_json::json!({
        "kind": "snapshot",
        "engineSource": "mysql",
        "engineTarget": "mysql",
        "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/db"},
        "extractor": {"extract_type": "cdc"},
    });

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "sync_mode_invalid_for_category");
}

// ═══════════════════════════════════════════════════════════════════════════
// RESOURCE GROUP CRUD
// ═════════════════════════════════════════════════════════════════════════

#[actix_web::test]
async fn list_resource_groups_includes_default() {
    let pool = test_pool().await;
    auth::seed_admin(&pool).await.unwrap();
    seed_default_rg(&pool).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_cookies(
        test::TestRequest::get().uri("/api/resource_groups"),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    let items = body.as_array().unwrap();
    assert!(!items.is_empty());
    assert!(items
        .iter()
        .any(|rg| rg["isDefault"] == true && rg["name"] == "default"));
}

#[actix_web::test]
async fn create_resource_group_201() {
    let pool = test_pool().await;
    auth::seed_admin(&pool).await.unwrap();
    seed_default_rg(&pool).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/resource_groups")
            .set_json(serde_json::json!({"name": "team-payments"})),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["name"], "team-payments");
    assert_eq!(body["isDefault"], false);
}

#[actix_web::test]
async fn duplicate_rg_name_409() {
    let pool = test_pool().await;
    auth::seed_admin(&pool).await.unwrap();
    seed_default_rg(&pool).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/resource_groups")
            .set_json(serde_json::json!({"name": "default"})),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "resource_group_name_taken");
}

#[actix_web::test]
async fn default_rg_cannot_be_deleted() {
    let pool = test_pool().await;
    auth::seed_admin(&pool).await.unwrap();
    seed_default_rg(&pool).await;

    let default_rg = ResourceGroupRepository::get_default(&pool).await.unwrap();

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::delete().uri(&format!("/api/resource_groups/{}", default_rg.id)),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "default_resource_group_protected");
}

#[actix_web::test]
async fn rg_with_tasks_cannot_be_deleted() {
    let pool = setup().await;

    // Create a custom RG with a task
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let custom_rg = ResourceGroup {
        id: uuid::Uuid::new_v4().to_string(),
        name: "team-a".to_string(),
        is_default: false,
        created_at: now.clone(),
        updated_at: now,
    };
    let custom_rg = ResourceGroupRepository::create(&pool, &custom_rg)
        .await
        .unwrap();

    let task = dt_console_server::models::Task {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: "snap_test".to_string(),
        name: "test".to_string(),
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
        resource_group_id: custom_rg.id.clone(),
        owner_user_id: None,
        status: "draft".to_string(),
        created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    };
    dt_console_server::repositories::task_repository::TaskRepository::create(&pool, &task)
        .await
        .unwrap();

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::delete().uri(&format!("/api/resource_groups/{}", custom_rg.id)),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "resource_group_has_tasks");
}

#[actix_web::test]
async fn delete_empty_rg_204() {
    let pool = test_pool().await;
    auth::seed_admin(&pool).await.unwrap();
    seed_default_rg(&pool).await;

    // Create custom RG (no tasks)
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let custom_rg = ResourceGroup {
        id: uuid::Uuid::new_v4().to_string(),
        name: "team-b".to_string(),
        is_default: false,
        created_at: now.clone(),
        updated_at: now,
    };
    let custom_rg = ResourceGroupRepository::create(&pool, &custom_rg)
        .await
        .unwrap();

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::delete().uri(&format!("/api/resource_groups/{}", custom_rg.id)),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[actix_web::test]
async fn unknown_rg_on_task_creation_422() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let mut body = snapshot_task_body();
    body["resourceGroupId"] = serde_json::json!("00000000-0000-0000-0000-000000000000");

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "unknown_resource_group");
}

// ═══════════════════════════════════════════════════════════════════════════
// SECURITY
// ═══════════════════════════════════════════════════════════════════════════

#[actix_web::test]
async fn path_traversal_blocked() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let mut body = check_task_body();
    body["sinker"]["check_log_dir"] = serde_json::json!("../../../etc/passwd");

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "path_outside_sandbox");
}

#[actix_web::test]
async fn ssrf_loopback_blocked() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let mut body = snapshot_task_body();
    body["sourceEndpoint"]["url"] = serde_json::json!("mysql://127.0.0.1:3306/db");

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "endpoint_host_blocked");
}

#[actix_web::test]
async fn ssrf_private_ip_blocked() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let mut body = snapshot_task_body();
    body["sourceEndpoint"]["url"] = serde_json::json!("mysql://10.0.0.5:3306/db");

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ═══════════════════════════════════════════════════════════════════════════
// PER-CATEGORY VALIDATION
// ═══════════════════════════════════════════════════════════════════════════

#[actix_web::test]
async fn cdc_mysql_missing_server_id_422() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let body = serde_json::json!({
        "kind": "cdc",
        "engineSource": "mysql",
        "engineTarget": "mysql",
        "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/db"},
        "extractor": {},
    });

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn check_missing_check_log_dir_422() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let body = serde_json::json!({
        "kind": "check",
        "engineSource": "mysql",
        "engineTarget": "mysql",
        "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/db"},
        "extractor": {},
        "sinker": {},
    });

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn struct_empty_filter_422() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let body = serde_json::json!({
        "kind": "struct",
        "engineSource": "mysql",
        "engineTarget": "mysql",
        "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/db"},
        "extractor": {"extract_type": "struct"},
        "sinker": {},
        "filter": {"do_dbs": [], "do_tbs": []},
    });

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "struct_filter_required");
}
