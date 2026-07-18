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
use dt_console_server::log_sse_handlers;
use dt_console_server::metrics_scraper;
use dt_console_server::middleware::csrf::{Csrf, XSRF_COOKIE_NAME, XSRF_HEADER_NAME};
use dt_console_server::models::{ControlLog, LoginRequest, ResourceGroup};
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use dt_console_server::repositories::control_log_repository::ControlLogRepository;
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

fn wizard_payload_contract(kind: &str) -> serde_json::Value {
    let contract: serde_json::Value = serde_json::from_str(include_str!(
        "../../web-prototype/tests/fixtures/checkStructWizardPayloadContract.json"
    ))
    .unwrap();
    contract[kind].clone()
}

fn check_task_body() -> serde_json::Value {
    let mut body = wizard_payload_contract("check");
    body["engineSource"] = serde_json::json!("mysql");
    body["engineTarget"] = serde_json::json!("mysql");
    body["sourceEndpoint"] = serde_json::json!({"url": "mysql://203.0.113.1:3306/src_db"});
    body["targetEndpoint"] = serde_json::json!({"url": "mysql://203.0.113.2:3306/dst_db"});
    body
}

fn struct_task_body() -> serde_json::Value {
    let mut body = wizard_payload_contract("struct");
    body["engineSource"] = serde_json::json!("mysql");
    body["engineTarget"] = serde_json::json!("mysql");
    body["sourceEndpoint"] = serde_json::json!({"url": "mysql://203.0.113.1:3306/src_db"});
    body["targetEndpoint"] = serde_json::json!({"url": "mysql://203.0.113.2:3306/dst_db"});
    body
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

/// Seed a non-admin user directly into the DB (bypasses API).
async fn seed_user(
    pool: &SqlitePool,
    username: &str,
    password: &str,
    role: &str,
    disabled: bool,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let hash = bcrypt::hash(password, 10).unwrap();
    let user = dt_console_server::models::User {
        id: id.clone(),
        username: username.to_string(),
        password_hash: hash,
        display_name: username.to_string(),
        role: role.to_string(),
        disabled,
        created_at: now.clone(),
        updated_at: now,
        resource_group_id: None,
    };
    dt_console_server::repositories::user_repository::UserRepository::create(pool, &user)
        .await
        .unwrap();
    id
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
    assert_eq!(body["sinker"], wizard_payload_contract("check")["sinker"]);
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
    assert_eq!(
        body["extractor"],
        wizard_payload_contract("struct")["extractor"]
    );
    assert_eq!(body["sinker"], wizard_payload_contract("struct")["sinker"]);
    assert_eq!(body["filter"], wizard_payload_contract("struct")["filter"]);
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
    assert_eq!(body["code"], "TASK_NOT_FOUND");
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
    assert_eq!(body["code"], "TASK_KIND_IMMUTABLE");
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
        task_id: Some(task_id.clone()),
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

    // Delete blocked
    let req = add_auth(
        test::TestRequest::delete().uri(&format!("/api/tasks/{task_id}")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "TASK_HAS_ACTIVE_RUN");
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

#[actix_web::test]
async fn list_tasks_migration_category_filters_by_mode() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let mut snapshot = snapshot_task_body();
    snapshot["name"] = serde_json::json!("snapshot-only");
    snapshot["extractor"] = serde_json::json!({"extract_type": "snapshot"});
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(snapshot),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let mut snapshot_cdc = snapshot_task_body();
    snapshot_cdc["name"] = serde_json::json!("snapshot-cdc");
    snapshot_cdc["extractor"] =
        serde_json::json!({"extract_type": "snapshot_and_cdc", "server_id": "2"});
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks")
            .set_json(snapshot_cdc),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let mut cdc = cdc_task_body();
    cdc["name"] = serde_json::json!("cdc-only");
    cdc["extractor"] = serde_json::json!({"extract_type": "cdc", "server_id": "3"});
    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(cdc),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let req = add_cookies(
        test::TestRequest::get().uri("/api/tasks?category=migration"),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total"], 3);
    let items = body["items"].as_array().unwrap();
    assert!(items.iter().all(|item| item["kind"] != "migration"));

    let req = add_cookies(
        test::TestRequest::get().uri("/api/tasks?category=migration&mode=snapshot"),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["name"], "snapshot-only");

    let req = add_cookies(
        test::TestRequest::get().uri("/api/tasks?category=migration&mode=snapshot_cdc"),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["name"], "snapshot-cdc");

    let req = add_cookies(
        test::TestRequest::get().uri("/api/tasks?category=migration&mode=cdc"),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["name"], "cdc-only");
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
    assert_eq!(body["code"], "GAUSSDB_SUB_MODE_REQUIRED");
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
async fn gaussdb_target_sub_mode_201() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let body = serde_json::json!({
        "kind": "snapshot",
        "engineSource": "oracle",
        "engineTarget": "gaussdb",
        "targetSubMode": "oracle-mode",
        "sourceEndpoint": {"url": "oracle://203.0.113.1:1521/db"},
        "targetEndpoint": {"url": "postgres://203.0.113.2:8000/db_ora_mode?sslmode=require&protocolVersion=351"},
        "extractor": {"extract_type": "snapshot_and_cdc", "cdc_mode": "logminer"},
    });

    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["dbTypeSource"], "oracle");
    assert_eq!(body["dbTypeTarget"], "gaussdb_oracle");
}

#[actix_web::test]
async fn patch_gaussdb_task_without_sub_mode_200() {
    // PATCH on an existing GaussDB task should NOT require sub_mode.
    // The bug was that is_gaussdb() matched resolved types like "gaussdb_pg",
    // causing PATCH to always fail with 422 gaussdb_sub_mode_required.
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create a GaussDB pg-mode snapshot task
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

    let created: serde_json::Value = test::read_body_json(resp).await;
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["dbTypeSource"], "gaussdb_pg");

    // PATCH without sub_mode — should succeed (not 422)
    let req = add_auth(
        test::TestRequest::patch()
            .uri(&format!("/api/tasks/{id}"))
            .set_json(serde_json::json!({
                "filter": {"do_tbs": ["db1.tbl_*"]}
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let patched: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(patched["dbTypeSource"], "gaussdb_pg");
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
    assert_eq!(body["code"], "SYNC_MODE_INVALID_FOR_CATEGORY");
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
    assert_eq!(body["code"], "RESOURCE_GROUP_NAME_TAKEN");
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
    assert_eq!(body["code"], "DEFAULT_RESOURCE_GROUP_PROTECTED");
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
    assert_eq!(body["code"], "RESOURCE_GROUP_HAS_TASKS");
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
    assert_eq!(body["code"], "UNKNOWN_RESOURCE_GROUP");
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
    assert_eq!(body["code"], "PATH_OUTSIDE_SANDBOX");
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
    assert_eq!(body["code"], "ENDPOINT_HOST_BLOCKED");
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
    assert_eq!(body["code"], "STRUCT_FILTER_REQUIRED");
}

// ── preview_ini, export, import, clone integration tests ────────────────────

#[actix_web::test]
async fn preview_ini_returns_ini_text() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create a task first
    let body = snapshot_task_body();
    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let task: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task["id"].as_str().unwrap();

    // GET preview_ini
    let req = add_auth(
        test::TestRequest::get().uri(&format!("/api/tasks/{task_id}/preview_ini")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let content_type = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("text/plain"));

    let body = test::read_body(resp).await;
    let ini_text = String::from_utf8(body.to_vec()).unwrap();
    assert!(ini_text.contains("[global]"));
    assert!(ini_text.contains("[extractor]"));
    assert!(ini_text.contains("[sinker]"));
    assert!(ini_text.contains("[filter]"));
    assert!(ini_text.contains("[parallelizer]"));
    assert!(ini_text.contains("[pipeline]"));
    assert!(ini_text.contains("[runtime]"));
    assert!(ini_text.contains("db_type=mysql"));
    assert!(ini_text.contains("extract_type=snapshot"));
}

#[actix_web::test]
async fn preview_ini_nonexistent_task_404() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_auth(
        test::TestRequest::get().uri("/api/tasks/00000000-0000-0000-0000-000000000000/preview_ini"),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn export_task_json() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let body = snapshot_task_body();
    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task["id"].as_str().unwrap();

    // GET export?format=json
    let req = add_auth(
        test::TestRequest::get().uri(&format!("/api/tasks/{task_id}/export?format=json")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let exported: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(exported["kind"], "snapshot");
    // Password should be redacted
    if let Some(pwd) = exported["sourceEndpoint"].get("password") {
        assert_eq!(pwd.as_str().unwrap(), "<redacted>");
    }
}

#[actix_web::test]
async fn export_task_ini() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let body = snapshot_task_body();
    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task["id"].as_str().unwrap();

    // GET export?format=ini
    let req = add_auth(
        test::TestRequest::get().uri(&format!("/api/tasks/{task_id}/export?format=ini")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = test::read_body(resp).await;
    let ini_text = String::from_utf8(body.to_vec()).unwrap();
    assert!(ini_text.contains("[extractor]"));
    assert!(ini_text.contains("db_type=mysql"));
}

#[actix_web::test]
async fn export_unsupported_format_400() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let body = snapshot_task_body();
    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task["id"].as_str().unwrap();

    let req = add_auth(
        test::TestRequest::get().uri(&format!("/api/tasks/{task_id}/export?format=yaml")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(err["code"], "UNSUPPORTED_EXPORT_FORMAT");
}

#[actix_web::test]
async fn import_task_from_json() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let import_body = serde_json::json!({
        "kind": "snapshot",
        "engineSource": "mysql",
        "engineTarget": "mysql",
        "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/db"},
        "extractor": {},
        "sinker": {},
        "filter": {"do_tbs": "test_db.*"},
    });

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks/import")
            .set_json(import_body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let task: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(task["kind"], "snapshot");
    assert_eq!(task["status"], "draft");
}

#[actix_web::test]
async fn import_task_validates_per_category() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Missing server_id for CDC mysql
    let import_body = serde_json::json!({
        "kind": "cdc",
        "engineSource": "mysql",
        "engineTarget": "mysql",
        "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/db"},
        "targetEndpoint": {"url": "mysql://203.0.113.2:3306/db"},
        "extractor": {"extract_type": "cdc"},
        "sinker": {},
    });

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks/import")
            .set_json(import_body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn import_batch_partial_report() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Batch: first valid, second invalid (struct without filter)
    let batch = serde_json::json!([
        {
            "kind": "snapshot",
            "engineSource": "mysql",
            "engineTarget": "mysql",
            "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/db"},
            "targetEndpoint": {"url": "mysql://203.0.113.2:3306/db"},
            "filter": {"do_tbs": "test.*"},
        },
        {
            "kind": "struct",
            "engineSource": "mysql",
            "engineTarget": "mysql",
            "sourceEndpoint": {"url": "mysql://203.0.113.1:3306/db"},
            "targetEndpoint": {"url": "mysql://203.0.113.2:3306/db"},
            "filter": {"do_dbs": [], "do_tbs": []},
        }
    ]);

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks/import")
            .set_json(batch),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let result: serde_json::Value = test::read_body_json(resp).await;
    assert!(!result["successes"].as_array().unwrap().is_empty());
    assert!(!result["failures"].as_array().unwrap().is_empty());
}

#[actix_web::test]
async fn clone_task_creates_independent_copy() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create original
    let body = snapshot_task_body();
    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let original: serde_json::Value = test::read_body_json(resp).await;
    let original_id = original["id"].as_str().unwrap();

    // Clone
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{original_id}/clone")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let cloned: serde_json::Value = test::read_body_json(resp).await;

    // Different id
    assert_ne!(cloned["id"], original["id"]);
    // Different task_id (has _copy_ suffix)
    assert!(cloned["taskId"].as_str().unwrap().contains("_copy_"));
    // Name has (copy) suffix
    assert!(cloned["name"].as_str().unwrap().contains("(copy)"));
    // Status is draft
    assert_eq!(cloned["status"], "draft");
    // Same kind
    assert_eq!(cloned["kind"], original["kind"]);
}

#[actix_web::test]
async fn clone_honours_license_cap() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Activate a license with maxTasks=1
    let expire = chrono::Utc::now() + chrono::Duration::days(365);
    let expire_str = expire.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let sig = dt_console_server::license_handlers::compute_signature(
        "professional",
        1,
        &expire_str,
        "test-org",
    );
    let payload = dt_console_server::models::ActivationPayload {
        sku: "professional".into(),
        max_tasks: 1,
        expire_at: expire_str,
        granted_to: "test-org".into(),
        sig,
    };
    let code = dt_console_server::license_handlers::generate_activation_code(&payload);
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/license/activate")
            .set_json(serde_json::json!({"code": code})),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Create one task (fills the cap)
    let body = snapshot_task_body();
    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let task: serde_json::Value = test::read_body_json(resp).await;

    // Clone should fail
    let task_id = task["id"].as_str().unwrap();
    let req = add_auth(
        test::TestRequest::post().uri(&format!("/api/tasks/{task_id}/clone")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert!(
        resp.status() == StatusCode::CONFLICT || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "clone at cap should return 409 or 422, got {}",
        resp.status()
    );
    let err: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(err["code"], "LICENSE_LIMIT_EXCEEDED");
}

#[actix_web::test]
async fn preview_ini_matches_renderer_output() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create a task
    let body = snapshot_task_body();
    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_resp: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_resp["id"].as_str().unwrap();

    // Load the task from DB and render in-process
    let db_task = dt_console_server::repositories::task_repository::TaskRepository::find_by_id(
        &pool, task_id,
    )
    .await
    .unwrap();
    let expected_ini = dt_console_server::ini_renderer::render(&db_task);

    // GET preview_ini
    let req = add_auth(
        test::TestRequest::get().uri(&format!("/api/tasks/{task_id}/preview_ini")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let body = test::read_body(resp).await;
    let actual_ini = String::from_utf8(body.to_vec()).unwrap();

    assert_eq!(
        actual_ini, expected_ini,
        "preview_ini must match IniRenderer::render output byte-for-byte"
    );
}

// ─── VAL-TASK-015: Task deletion with FK cascade ────────────────────────

/// After deleting a task that has stopped runs and control_logs referencing it,
/// deletion succeeds, run.task_id becomes NULL, and control_logs rows are
/// still queryable by the old task_id (denormalised audit).
#[actix_web::test]
async fn task_delete_cascades_fk_set_null() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create a task
    let body = snapshot_task_body();
    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_resp: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_resp["id"].as_str().unwrap().to_string();

    // Insert a stopped run directly into the DB (no active run → deletion allowed)
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let run = dt_console_server::models::Run {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: Some(task_id.clone()),
        status: "stopped".to_string(),
        pid: None,
        ini_path: Some("/tmp/ini".to_string()),
        log_dir: Some("/tmp/logs".to_string()),
        started_at: Some(now.clone()),
        stopped_at: Some(now.clone()),
        exit_code: Some(0),
        stop_method: Some("graceful".to_string()),
        metrics_port: None,
        created_at: now.clone(),
        updated_at: now,
    };
    RunRepository::create(&pool, &run).await.unwrap();
    let run_id = run.id.clone();

    // Insert a control_log referencing the task
    let ctrl = ControlLog {
        id: 0,
        task_id: task_id.clone(),
        run_id: Some(run_id.clone()),
        action: "start".to_string(),
        intent_or_result: "result:success".to_string(),
        operator_id: Some("admin".to_string()),
        created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    };
    ControlLogRepository::create(&pool, &ctrl).await.unwrap();

    // Delete the task → should succeed (no active run)
    let req = add_auth(
        test::TestRequest::delete().uri(&format!("/api/tasks/{task_id}")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "task deletion should succeed"
    );

    // GET /api/tasks/:id → 404
    let req = add_auth(
        test::TestRequest::get().uri(&format!("/api/tasks/{task_id}")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "deleted task should return 404"
    );

    // Verify run.task_id is now NULL (SET NULL cascade)
    let updated_run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
    assert!(
        updated_run.task_id.is_none(),
        "run.task_id should be NULL after task deletion (ON DELETE SET NULL)"
    );

    // Verify control_logs rows are still queryable by the old task_id (denormalised)
    let (logs, total) = ControlLogRepository::list_filtered(
        &pool,
        &dt_console_server::repositories::control_log_repository::ControlLogFilter {
            task_id: Some(&task_id),
            action: None,
            from: None,
            to: None,
            run_id: None,
            page: 1,
            page_size: 10,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        total, 1,
        "control_logs should still be queryable by old task_id"
    );
    assert_eq!(
        logs[0].task_id, task_id,
        "control_log.task_id preserved (denormalised)"
    );
}

/// Deleting a task with a running run is still blocked (409).
#[actix_web::test]
async fn task_delete_blocked_by_active_run_unchanged() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create a task
    let body = snapshot_task_body();
    let req = add_auth(
        test::TestRequest::post().uri("/api/tasks").set_json(body),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    let task_resp: serde_json::Value = test::read_body_json(resp).await;
    let task_id = task_resp["id"].as_str().unwrap().to_string();

    // Insert a RUNNING run directly
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let run = dt_console_server::models::Run {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: Some(task_id.clone()),
        status: "running".to_string(),
        pid: Some(1234),
        ini_path: Some("/tmp/ini".to_string()),
        log_dir: Some("/tmp/logs".to_string()),
        started_at: Some(now.clone()),
        stopped_at: None,
        exit_code: None,
        stop_method: None,
        metrics_port: None,
        created_at: now.clone(),
        updated_at: now,
    };
    RunRepository::create(&pool, &run).await.unwrap();

    // DELETE should fail with 409
    let req = add_auth(
        test::TestRequest::delete().uri(&format!("/api/tasks/{task_id}")),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "active run should block deletion"
    );
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "TASK_HAS_ACTIVE_RUN");
}

// ─── VAL-INTEG-004: Preview endpoints accept body without persistence ────

/// POST /api/tasks/preview/test_connection with a CreateTaskRequest body
/// returns per-side results without creating any DB rows.
#[actix_web::test]
async fn preview_test_connection_no_persistence() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Count tasks before
    let task_count_before =
        dt_console_server::repositories::task_repository::TaskRepository::count(&pool)
            .await
            .unwrap();

    // Call preview test_connection
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks/preview/test_connection")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://root:@127.0.0.1:19999/test" },
                "targetEndpoint": { "url": "mysql://root:@127.0.0.1:19998/test" },
                "extractor": { "extractType": "snapshot" },
                "sinker": { "sinkType": "write" }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    // 200 even on connection failure (per-side results)
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "preview test_connection should return 200"
    );

    // Verify no new rows in tasks
    let task_count_after =
        dt_console_server::repositories::task_repository::TaskRepository::count(&pool)
            .await
            .unwrap();
    assert_eq!(
        task_count_before, task_count_after,
        "preview test_connection must not persist tasks"
    );
}

/// POST /api/tasks/preview/precheck with a CreateTaskRequest body
/// returns precheck items without creating any DB rows.
#[actix_web::test]
async fn preview_precheck_no_persistence() {
    let pool = setup().await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Count tasks before
    let task_count_before =
        dt_console_server::repositories::task_repository::TaskRepository::count(&pool)
            .await
            .unwrap();

    // Call preview precheck
    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks/preview/precheck")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://root:@127.0.0.1:19999/test" },
                "targetEndpoint": { "url": "mysql://root:@127.0.0.1:19998/test" },
                "extractor": { "extractType": "snapshot" },
                "sinker": { "sinkType": "write" }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    // 200 even on check failure
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "preview precheck should return 200"
    );

    // Verify no new rows in tasks
    let task_count_after =
        dt_console_server::repositories::task_repository::TaskRepository::count(&pool)
            .await
            .unwrap();
    assert_eq!(
        task_count_before, task_count_after,
        "preview precheck must not persist tasks"
    );
}

/// Viewer cannot use preview test_connection (403).
#[actix_web::test]
async fn preview_test_connection_viewer_forbidden() {
    let pool = setup().await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "viewer1", "view123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks/preview/test_connection")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://root:@127.0.0.1:19999/test" },
                "targetEndpoint": { "url": "mysql://root:@127.0.0.1:19998/test" },
                "extractor": { "extractType": "snapshot" },
                "sinker": { "sinkType": "write" }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "viewer should be forbidden from preview test_connection"
    );
}

/// Viewer cannot use preview precheck (403).
#[actix_web::test]
async fn preview_precheck_viewer_forbidden() {
    let pool = setup().await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;
    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "viewer1", "view123");

    let req = add_auth(
        test::TestRequest::post()
            .uri("/api/tasks/preview/precheck")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
                "sourceEndpoint": { "url": "mysql://root:@127.0.0.1:19999/test" },
                "targetEndpoint": { "url": "mysql://root:@127.0.0.1:19998/test" },
                "extractor": { "extractType": "snapshot" },
                "sinker": { "sinkType": "write" }
            })),
        &cookies,
    )
    .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "viewer should be forbidden from preview precheck"
    );
}
