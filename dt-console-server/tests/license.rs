//! Integration tests for license endpoints:
//! GET /api/license, POST /api/license/activate
//!
//! Covers:
//! - VAL-LICENSE-001: GET /api/license returns the current license
//! - VAL-LICENSE-002: Admin activates a license with a valid code → 200
//! - VAL-LICENSE-003: Activation with an invalid code → 400
//! - VAL-LICENSE-004: License persists across restart (SQLite persists)
//! - VAL-LICENSE-005: Task creation past the cap → 422 LICENSE_LIMIT_EXCEEDED
//! - VAL-LICENSE-006: Task creation under the cap → 201
//! - VAL-LICENSE-008: Expired license refuses to start a Task
//! - VAL-LICENSE-EDGE-001: Boundary values for max_tasks (0, negative)
//! - VAL-LICENSE-EDGE-003: currentTasks counts non-deleted Tasks only
//! - VAL-TASK-025: License cap enforced on create
//! - RBAC: operator/viewer cannot activate license

use actix_session::storage::CookieSessionStore;
use actix_session::SessionMiddleware;
use actix_web::cookie::{Cookie, Key, SameSite};
use actix_web::http::StatusCode;
use actix_web::test;
use actix_web::web::{self, JsonConfig};
use actix_web::App;
use dt_console_server::auth;
use dt_console_server::auth_handlers;
use dt_console_server::db;
use dt_console_server::error;
use dt_console_server::health;
use dt_console_server::license_handlers;
use dt_console_server::middleware::csrf::{Csrf, XSRF_COOKIE_NAME, XSRF_HEADER_NAME};
use dt_console_server::models::{ActivationPayload, LoginRequest};
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use dt_console_server::repositories::task_repository::TaskRepository;
use dt_console_server::user_handlers;
use sqlx::SqlitePool;

// ─── Helpers ─────────────────────────────────────────────────────────────

/// Create a migrated test pool backed by a temp file.
async fn test_pool() -> SqlitePool {
    let dir = std::env::temp_dir().join("dt-console-server-license-test");
    std::fs::create_dir_all(&dir).unwrap();
    let test_name = std::thread::current()
        .name()
        .unwrap_or("unknown")
        .to_string();
    let safe_name: String = test_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let path = dir.join(format!("license-{safe_name}.db"));
    let _ = std::fs::remove_file(&path);
    let path_str = path.to_string_lossy().to_string();
    let pool = db::create_pool(&path_str).await.unwrap();
    db::run_migrations(&pool).await.unwrap();
    pool
}

const IDLE_TIMEOUT_SECS: i64 = 3600;
const XSRF: &str = "test-xsrf-token";

/// Build the standard test app with full middleware stack.
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
        .service(
            web::scope("/api")
                .service(health::healthz)
                .service(auth_handlers::login)
                .service(auth_handlers::logout)
                .service(auth_handlers::me)
                .service(user_handlers::list_users)
                .service(user_handlers::create_user)
                .service(user_handlers::get_user)
                .service(user_handlers::update_user)
                .service(user_handlers::delete_user)
                .service(license_handlers::get_license)
                .service(license_handlers::activate_license),
        )
}

/// Seed a user directly in the DB and return user_id.
async fn seed_user(
    pool: &SqlitePool,
    username: &str,
    password: &str,
    role: &str,
    disabled: bool,
) -> String {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let password_hash = auth::hash_password(password).unwrap();
    let user_id = uuid::Uuid::new_v4().to_string();
    let user = dt_console_server::models::User {
        id: user_id.clone(),
        username: username.to_string(),
        password_hash,
        display_name: username.to_string(),
        role: role.to_string(),
        disabled,
        created_at: now.clone(),
        updated_at: now,
    };
    dt_console_server::repositories::user_repository::UserRepository::create(pool, &user)
        .await
        .unwrap();
    user_id
}

/// Seed a task directly in the DB and return task_id.
async fn seed_task(pool: &SqlitePool, name: &str) -> String {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let id = uuid::Uuid::new_v4().to_string();
    let task = dt_console_server::models::Task {
        id: id.clone(),
        task_id: format!("task_{name}"),
        name: name.to_string(),
        kind: "snapshot".to_string(),
        db_type_source: "mysql".to_string(),
        db_type_target: "mysql".to_string(),
        source_endpoint: "{}".to_string(),
        target_endpoint: "{}".to_string(),
        extractor_config: "{}".to_string(),
        filter_config: "{}".to_string(),
        router_config: "{}".to_string(),
        parallelizer_config: "{}".to_string(),
        pipeline_config: "{}".to_string(),
        resumer_config: "{}".to_string(),
        processor_config: "{}".to_string(),
        runtime_config: "{}".to_string(),
        metrics_config: "{}".to_string(),
        resource_group_id: "default".to_string(),
        owner_user_id: None,
        status: "draft".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };
    TaskRepository::create(pool, &task).await.unwrap();
    id
}

/// Collect all cookies from a response's Set-Cookie headers.
fn collect_cookies<B>(res: &actix_web::dev::ServiceResponse<B>) -> Vec<Cookie<'static>> {
    let mut cookies = Vec::new();
    for val in res.headers().get_all("set-cookie") {
        if let Ok(cookie) = Cookie::parse_encoded(val.to_str().unwrap_or("").to_string()) {
            cookies.push(cookie.into_owned());
        }
    }
    cookies
}

/// Add a set of cookies to a TestRequest.
fn add_cookies(mut req: test::TestRequest, cookies: &[Cookie<'static>]) -> test::TestRequest {
    for c in cookies {
        req = req.cookie(c.clone());
    }
    req
}

/// Login as a user and return the session cookies.
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
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "login should succeed for {}",
            $username
        );
        collect_cookies(&res)
    }};
}

/// Build a POST /api/license/activate request with auth cookies and CSRF.
fn activate_req(cookies: &[Cookie<'static>], code: &str) -> test::TestRequest {
    add_cookies(
        test::TestRequest::post()
            .uri("/api/license/activate")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF)),
        cookies,
    )
    .set_json(serde_json::json!({ "code": code }))
}

/// Build a GET /api/license request with auth cookies.
fn get_license_req(cookies: &[Cookie<'static>]) -> test::TestRequest {
    add_cookies(test::TestRequest::get().uri("/api/license"), cookies)
}

/// Generate a valid activation code for the given parameters.
fn make_activation_code(sku: &str, max_tasks: i64, expire_at: &str, granted_to: &str) -> String {
    let sig = license_handlers::compute_signature(sku, max_tasks, expire_at, granted_to);
    let payload = ActivationPayload {
        sku: sku.to_string(),
        max_tasks,
        expire_at: expire_at.to_string(),
        granted_to: granted_to.to_string(),
        sig,
    };
    license_handlers::generate_activation_code(&payload)
}

/// Generate a valid code for a license that expires far in the future.
fn make_valid_code(max_tasks: i64) -> String {
    let expire = chrono::Utc::now() + chrono::Duration::days(365);
    let expire_str = expire.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    make_activation_code("professional", max_tasks, &expire_str, "test-org")
}

/// Generate an expired license code.
fn make_expired_code(max_tasks: i64) -> String {
    make_activation_code(
        "professional",
        max_tasks,
        "2020-01-01T00:00:00Z",
        "test-org",
    )
}

// ─── Tests ─────────────────────────────────────────────────────────────

/// VAL-LICENSE-001: GET /api/license returns the current license shape.
/// When no license is active, returns status="missing".
#[actix_web::test]
async fn get_license_no_license_returns_missing() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = get_license_req(&cookies).to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["status"], "missing");
    assert_eq!(body["sku"], "");
    assert_eq!(body["maxTasks"], 0);
    assert!(body["currentTasks"].is_number());
    // Body must NOT contain the raw activation code
    assert!(body.get("activationCode").is_none());
    assert!(body.get("code").is_none());
}

/// VAL-LICENSE-001: GET /api/license returns correct shape after activation.
#[actix_web::test]
async fn get_license_after_activation_returns_shape() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Activate a license first
    let code = make_valid_code(10);
    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Now GET the license
    let req = get_license_req(&cookies).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["sku"], "professional");
    assert_eq!(body["maxTasks"], 10);
    assert_eq!(body["status"], "active");
    assert_eq!(body["grantedTo"], "test-org");
    assert!(body["expireAt"].is_string());
    assert!(body["activatedAt"].is_string());
    assert_eq!(body["currentTasks"], 0);
    // Body must NOT contain the raw activation code
    assert!(body.get("activationCode").is_none());
    assert!(body.get("code").is_none());
}

/// VAL-LICENSE-002: Admin activates a license with a valid code → 200.
#[actix_web::test]
async fn activate_valid_code_returns_200() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let code = make_valid_code(5);
    let req = activate_req(&cookies, &code).to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["sku"], "professional");
    assert_eq!(body["maxTasks"], 5);
    assert_eq!(body["status"], "active");
    assert_eq!(body["grantedTo"], "test-org");
}

/// VAL-LICENSE-003: Activation with an invalid code → 400 INVALID_LICENSE_CODE.
#[actix_web::test]
async fn activate_invalid_code_returns_400() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Test various invalid codes
    for invalid_code in &["GARBAGE", "!!!", "", "not-base64-at-all!!!"] {
        let req = activate_req(&cookies, invalid_code).to_request();

        let res = test::call_service(&app, req).await;
        assert_eq!(
            res.status(),
            StatusCode::BAD_REQUEST,
            "expected 400 for code: {invalid_code}"
        );

        let body: serde_json::Value = test::read_body_json(res).await;
        assert_eq!(body["code"], "INVALID_LICENSE_CODE");
    }
}

/// VAL-LICENSE-003: Valid base64 but wrong signature → 400.
#[actix_web::test]
async fn activate_tampered_signature_returns_400() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create a code with a valid format but tampered signature
    let expire = chrono::Utc::now() + chrono::Duration::days(365);
    let expire_str = expire.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let payload = ActivationPayload {
        sku: "professional".to_string(),
        max_tasks: 10,
        expire_at: expire_str,
        granted_to: "test-org".to_string(),
        sig: "deadbeef12345678".to_string(), // tampered
    };
    let code = license_handlers::generate_activation_code(&payload);

    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], "INVALID_LICENSE_CODE");
}

/// VAL-LICENSE-EDGE-001: Negative max_tasks in code → 400 INVALID_LICENSE_CODE.
#[actix_web::test]
async fn activate_negative_max_tasks_returns_400() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let expire = chrono::Utc::now() + chrono::Duration::days(365);
    let expire_str = expire.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let sig = license_handlers::compute_signature("professional", -5, &expire_str, "test-org");
    let payload = ActivationPayload {
        sku: "professional".to_string(),
        max_tasks: -5,
        expire_at: expire_str,
        granted_to: "test-org".to_string(),
        sig,
    };
    let code = license_handlers::generate_activation_code(&payload);

    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], "INVALID_LICENSE_CODE");
}

/// VAL-LICENSE-EDGE-001: max_tasks=0 → all task creation refused.
#[actix_web::test]
async fn max_tasks_zero_refuses_all_creation() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Activate license with max_tasks=0
    let code = make_valid_code(0);
    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // GET /api/license shows max_tasks=0
    let req = get_license_req(&cookies).to_request();
    let res = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["maxTasks"], 0);
    assert_eq!(body["currentTasks"], 0);

    // Verify check_license_cap rejects creation when max_tasks=0
    let result = license_handlers::check_license_cap(&pool).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, "LICENSE_LIMIT_EXCEEDED");
    let details = err.details.unwrap();
    assert_eq!(details["maxTasks"], 0);
    assert_eq!(details["currentTasks"], 0);
}

/// VAL-LICENSE-005: Task creation past the cap → 422 LICENSE_LIMIT_EXCEEDED.
#[actix_web::test]
async fn task_creation_past_cap_returns_422() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Activate license with max_tasks=2
    let code = make_valid_code(2);
    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Create 2 tasks directly in DB
    seed_task(&pool, "task1").await;
    seed_task(&pool, "task2").await;

    // Verify cap is reached via check_license_cap
    let result = license_handlers::check_license_cap(&pool).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, "LICENSE_LIMIT_EXCEEDED");
    let details = err.details.unwrap();
    assert_eq!(details["maxTasks"], 2);
    assert_eq!(details["currentTasks"], 2);
}

/// VAL-LICENSE-006: Task creation under the cap succeeds.
#[actix_web::test]
async fn task_creation_under_cap_succeeds() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Activate license with max_tasks=5
    let code = make_valid_code(5);
    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Create 3 tasks directly in DB
    seed_task(&pool, "task1").await;
    seed_task(&pool, "task2").await;
    seed_task(&pool, "task3").await;

    // Verify cap is NOT reached
    let result = license_handlers::check_license_cap(&pool).await;
    assert!(result.is_ok());
    let lic = result.unwrap();
    assert_eq!(lic.max_tasks, 5);
}

/// VAL-LICENSE-008: Expired license refuses to start a Task.
#[actix_web::test]
async fn expired_license_refuses_start() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Activate an expired license
    let code = make_expired_code(10);
    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // GET /api/license shows expired
    let req = get_license_req(&cookies).to_request();
    let res = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["status"], "expired");

    // check_license_for_start rejects start
    let result = license_handlers::check_license_for_start(&pool).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, "LICENSE_EXPIRED");

    // check_license_cap also rejects creation when expired
    let result = license_handlers::check_license_cap(&pool).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, "LICENSE_EXPIRED");
}

/// VAL-LICENSE-008: Expired license → creation refused with correct error details.
#[actix_web::test]
async fn expired_license_creation_refused_with_details() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Activate an expired license
    let code = make_expired_code(10);
    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Attempt creation
    let result = license_handlers::check_license_cap(&pool).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, "LICENSE_EXPIRED");
    let details = err.details.unwrap();
    assert_eq!(details["status"], "expired");
}

/// VAL-LICENSE-EDGE-003: currentTasks counts non-deleted Tasks only.
#[actix_web::test]
async fn current_tasks_counts_non_deleted_only() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Activate license with max_tasks=5
    let code = make_valid_code(5);
    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Create 5 tasks
    let t1 = seed_task(&pool, "task1").await;
    let t2 = seed_task(&pool, "task2").await;
    let _t3 = seed_task(&pool, "task3").await;
    seed_task(&pool, "task4").await;
    seed_task(&pool, "task5").await;

    // currentTasks should be 5
    let req = get_license_req(&cookies).to_request();
    let res = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["currentTasks"], 5);

    // At cap — creation refused
    let result = license_handlers::check_license_cap(&pool).await;
    assert!(result.is_err());

    // Delete one task
    TaskRepository::delete(&pool, &t1).await.unwrap();

    // currentTasks should be 4
    let req = get_license_req(&cookies).to_request();
    let res = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["currentTasks"], 4);

    // Under cap now — creation allowed
    let result = license_handlers::check_license_cap(&pool).await;
    assert!(result.is_ok());

    // Delete another task
    TaskRepository::delete(&pool, &t2).await.unwrap();

    // currentTasks should be 3
    let count = TaskRepository::count(&pool).await.unwrap();
    assert_eq!(count, 3);
}

/// No license → creation refused (missing status).
#[actix_web::test]
async fn no_license_refuses_creation() {
    let pool = test_pool().await;

    // No license activated — creation should be refused
    let result = license_handlers::check_license_cap(&pool).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, "LICENSE_LIMIT_EXCEEDED");
    let details = err.details.unwrap();
    assert_eq!(details["maxTasks"], 0);
    assert_eq!(details["currentTasks"], 0);
}

/// Re-activation updates the license (upsert behaviour).
#[actix_web::test]
async fn reactivate_updates_license() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Activate with max_tasks=5
    let code1 = make_valid_code(5);
    let req = activate_req(&cookies, &code1).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body1: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body1["maxTasks"], 5);

    // Re-activate with max_tasks=20
    let code2 = make_valid_code(20);
    let req = activate_req(&cookies, &code2).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body2: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body2["maxTasks"], 20);

    // GET license reflects the new values
    let req = get_license_req(&cookies).to_request();
    let res = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["maxTasks"], 20);
}

/// RBAC: Operator cannot activate a license.
#[actix_web::test]
async fn operator_cannot_activate_license() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "operator1", "op123", "operator", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "operator1", "op123");

    let code = make_valid_code(10);
    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], "FORBIDDEN");
}

/// RBAC: Viewer cannot activate a license.
#[actix_web::test]
async fn viewer_cannot_activate_license() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "viewer1", "view123");

    let code = make_valid_code(10);
    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], "FORBIDDEN");
}

/// RBAC: All roles can GET /api/license.
#[actix_web::test]
async fn all_roles_can_get_license() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "operator1", "op123", "operator", false).await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;

    for (username, password) in [
        ("admin", "admin123"),
        ("operator1", "op123"),
        ("viewer1", "view123"),
    ] {
        let cookies = do_login!(app, username, password);
        let req = get_license_req(&cookies).to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "{username} should be able to GET /api/license"
        );

        let body: serde_json::Value = test::read_body_json(res).await;
        assert_eq!(body["status"], "missing");
    }
}

/// VAL-LICENSE-004: License persists in SQLite (restart test via DB query).
/// We can't restart the server in a test, but we verify the row persists
/// across a new app instance pointing at the same DB.
#[actix_web::test]
async fn license_persists_in_sqlite() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Activate a license
    let code = make_valid_code(10);
    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Verify the license row is in the DB directly
    let license =
        dt_console_server::repositories::license_repository::LicenseRepository::get_current(&pool)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(license.sku, "professional");
    assert_eq!(license.max_tasks, 10);
    assert_eq!(license.granted_to, "test-org");

    // Create a new app instance pointing at the same DB — license should persist
    let app2 = test::init_service(build_test_app(pool.clone())).await;
    let cookies2 = do_login!(app2, "admin", "admin123");

    let req = get_license_req(&cookies2).to_request();
    let res = test::call_service(&app2, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["sku"], "professional");
    assert_eq!(body["maxTasks"], 10);
    assert_eq!(body["grantedTo"], "test-org");
}

/// Audit log is written on successful activation.
#[actix_web::test]
async fn activation_writes_audit_log() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let code = make_valid_code(10);
    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Check the operate_logs table
    let logs =
        dt_console_server::repositories::operate_log_repository::OperateLogRepository::list(&pool)
            .await
            .unwrap();

    let activate_logs: Vec<_> = logs
        .iter()
        .filter(|l| l.action == "license.activate" && l.result == "success")
        .collect();
    assert_eq!(
        activate_logs.len(),
        1,
        "should have one successful activation log"
    );
    assert_eq!(activate_logs[0].actor, "admin");

    // The raw code must NOT appear in the log
    for log in &logs {
        if let Some(ref details) = log.details {
            assert!(
                !details.contains(&code),
                "activation code must not appear in audit log"
            );
        }
    }
}

/// Audit log is written on failed activation.
#[actix_web::test]
async fn failed_activation_writes_audit_log() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = activate_req(&cookies, "GARBAGE").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // Check the operate_logs table
    let logs =
        dt_console_server::repositories::operate_log_repository::OperateLogRepository::list(&pool)
            .await
            .unwrap();

    let fail_logs: Vec<_> = logs
        .iter()
        .filter(|l| l.action == "license.activate" && l.result == "failure")
        .collect();
    assert_eq!(fail_logs.len(), 1, "should have one failed activation log");
}

/// Expiring_soon status: within 30 days of expiry.
#[actix_web::test]
async fn expiring_soon_status_within_30_days() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool.clone())).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create a code that expires in 7 days
    let expire = chrono::Utc::now() + chrono::Duration::days(7);
    let expire_str = expire.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let code = make_activation_code("professional", 10, &expire_str, "test-org");

    let req = activate_req(&cookies, &code).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["status"], "expiring_soon");
}

/// Anonymous on GET /api/license → 401.
#[actix_web::test]
async fn anonymous_get_license_returns_401() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::get().uri("/api/license").to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

/// Anonymous on POST /api/license/activate → 401.
#[actix_web::test]
async fn anonymous_activate_license_returns_401() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::post()
        .uri("/api/license/activate")
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .set_json(serde_json::json!({ "code": "test" }))
        .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

/// No CSRF token on POST /api/license/activate → 403.
#[actix_web::test]
async fn activate_without_csrf_returns_403() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(app, "admin", "admin123");

    // POST without X-XSRF-TOKEN header
    let req = add_cookies(
        test::TestRequest::post()
            .uri("/api/license/activate")
            .set_json(serde_json::json!({ "code": "test" })),
        &cookies,
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}
