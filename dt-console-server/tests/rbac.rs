//! RBAC middleware integration tests: per-endpoint role enforcement.
//!
//! Covers:
//! - VAL-RBAC-001: Admin can perform every action
//! - VAL-RBAC-002: Operator can perform their allowed actions
//! - VAL-RBAC-003: All roles can read tasks (GET /api/tasks is not yet built,
//!   but the role matrix is tested via middleware::rbac unit tests)
//! - VAL-RBAC-004: Anonymous on protected endpoints → 401, not 403
//! - VAL-RBAC-005: Viewer cannot create tasks → 403
//! - VAL-RBAC-006: Operator cannot delete tasks → 403
//! - VAL-RBAC-007: Viewer cannot delete tasks → 403
//! - VAL-RBAC-008: Viewer cannot start a task → 403
//! - VAL-RBAC-009: Viewer cannot stop a task → 403
//! - VAL-RBAC-010: Operator cannot manage users → 403
//! - VAL-RBAC-011: Viewer cannot manage users → 403
//! - VAL-RBAC-012: Operator cannot activate a license → 403
//! - VAL-RBAC-013: Viewer cannot activate a license → 403
//! - VAL-RBAC-014: Viewer cannot clear alerts → 403 (alert endpoints not built yet)
//! - VAL-RBAC-015: Server-side enforcement: curl with viewer cookie → 403
//! - VAL-SEC-INJ-001: SQL injection on filter endpoints prevented by parameterisation
//!
//! Task lifecycle endpoints (start/stop/delete) are not yet built — those
//! assertions are covered by the middleware::rbac unit tests and will be
//! exercised in integration when the endpoints are added.

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
use dt_console_server::log_sse_handlers;
use dt_console_server::metrics_scraper;
use dt_console_server::middleware::csrf::{Csrf, XSRF_COOKIE_NAME, XSRF_HEADER_NAME};
use dt_console_server::models::LoginRequest;
use dt_console_server::operate_log_handlers;
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use dt_console_server::user_handlers;
use sqlx::SqlitePool;

// ─── Helpers ─────────────────────────────────────────────────────────────

/// Create a migrated test pool backed by a temp file.
async fn test_pool() -> SqlitePool {
    let dir = std::env::temp_dir().join("dt-console-server-rbac-test");
    std::fs::create_dir_all(&dir).unwrap();
    let test_name = std::thread::current()
        .name()
        .unwrap_or("unknown")
        .to_string();
    let safe_name: String = test_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let path = dir.join(format!("rbac-{safe_name}.db"));
    let _ = std::fs::remove_file(&path);
    let path_str = path.to_string_lossy().to_string();
    let pool = db::create_pool(&path_str).await.unwrap();
    db::run_migrations(&pool).await.unwrap();
    pool
}

const IDLE_TIMEOUT_SECS: i64 = 3600;
const XSRF: &str = "test-xsrf-token";

/// Build the standard test app with full middleware stack including all
/// current endpoints.
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
        .app_data(web::Data::new(
            dt_console_server::sse_session_tracker::SseSessionTracker::new(),
        ))
        .app_data(web::Data::new(RateLimiter::new(RateLimitConfig::default())))
        .app_data(web::Data::new(IDLE_TIMEOUT_SECS))
        .app_data(web::Data::new(metrics_scraper::ScraperState::new()))
        .app_data(web::Data::new(log_sse_handlers::LogSseState::default()))
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
                .service(operate_log_handlers::list_operate_logs)
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
        resource_group_id: None,
        created_at: now.clone(),
        updated_at: now,
    };
    dt_console_server::repositories::user_repository::UserRepository::create(pool, &user)
        .await
        .unwrap();
    user_id
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

/// Login as a user and return the session cookies.
macro_rules! do_login {
    ($app:expr, $username:expr, $password:expr) => {{
        let req = test::TestRequest::post()
            .uri("/api/auth/login")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
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

/// Add a set of cookies to a TestRequest.
fn add_cookies(mut req: test::TestRequest, cookies: &[Cookie<'static>]) -> test::TestRequest {
    for c in cookies {
        req = req.cookie(c.clone());
    }
    req
}

/// Helper: assert that a 403 response has the expected required_action.
macro_rules! assert_forbidden {
    ($res:expr, $expected_action:expr) => {
        let status = $res.status();
        assert_eq!(status, StatusCode::FORBIDDEN);
        let body: serde_json::Value = test::read_body_json($res).await;
        assert_eq!(body["code"], "FORBIDDEN");
        assert_eq!(body["details"]["required_action"], $expected_action);
    };
}

/// Generate a valid license activation code.
fn make_valid_code(max_tasks: i64) -> String {
    let sig = dt_console_server::license_handlers::compute_signature(
        "pro",
        max_tasks,
        "2099-12-31T23:59:59Z",
        "test-org",
    );
    let payload = dt_console_server::models::ActivationPayload {
        sku: "pro".to_string(),
        max_tasks,
        expire_at: "2099-12-31T23:59:59Z".to_string(),
        granted_to: "test-org".to_string(),
        sig,
    };
    dt_console_server::license_handlers::generate_activation_code(&payload)
}

// ─── VAL-RBAC-004: Anonymous on protected endpoints → 401, not 403 ────

#[actix_web::test]
async fn anonymous_get_users_returns_401_not_403() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::get()
        .uri("/api/users")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .to_request();
    let res = test::call_service(&app, req).await;
    let status = res.status();
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], "UNAUTHENTICATED");
    // Must NOT be 403
    assert_ne!(status, StatusCode::FORBIDDEN);
}

#[actix_web::test]
async fn anonymous_post_users_returns_401_not_403() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::post()
        .uri("/api/users")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(serde_json::json!({
            "username": "newuser",
            "password": "pass123",
            "role": "viewer"
        }))
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], "UNAUTHENTICATED");
}

#[actix_web::test]
async fn anonymous_delete_users_returns_401_not_403() {
    let pool = test_pool().await;
    let uid = seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "bob", "bob123", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::delete()
        .uri(&format!("/api/users/{uid}"))
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn anonymous_activate_license_returns_401_not_403() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let code = make_valid_code(10);
    let req = test::TestRequest::post()
        .uri("/api/license/activate")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(serde_json::json!({ "code": code }))
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], "UNAUTHENTICATED");
}

#[actix_web::test]
async fn anonymous_get_operate_logs_returns_401_not_403() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::get()
        .uri("/api/operate_logs")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

// ─── VAL-RBAC-010: Operator cannot manage users → 403 ──────────────────

#[actix_web::test]
async fn operator_cannot_list_users() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "operator1", "op123", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "operator1", "op123");
    let req = add_cookies(test::TestRequest::get().uri("/api/users"), &cookies).to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "users.list");
}

#[actix_web::test]
async fn operator_cannot_create_user() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "operator1", "op123", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "operator1", "op123");
    let req = add_cookies(
        test::TestRequest::post()
            .uri("/api/users")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(serde_json::json!({
                "username": "newuser",
                "password": "pass123",
                "role": "viewer"
            })),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "users.create");
}

#[actix_web::test]
async fn operator_cannot_get_user() {
    let pool = test_pool().await;
    let uid = seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "operator1", "op123", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "operator1", "op123");
    let req = add_cookies(
        test::TestRequest::get().uri(&format!("/api/users/{uid}")),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "users.read");
}

#[actix_web::test]
async fn operator_cannot_patch_user() {
    let pool = test_pool().await;
    let uid = seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "operator1", "op123", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "operator1", "op123");
    let req = add_cookies(
        test::TestRequest::patch()
            .uri(&format!("/api/users/{uid}"))
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(serde_json::json!({ "displayName": "hacked" })),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "users.update");
}

#[actix_web::test]
async fn operator_cannot_delete_user() {
    let pool = test_pool().await;
    // Need 2 admins so deletion of 1 doesn't violate last-admin invariant
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let uid = seed_user(&pool, "admin2", "admin2123", "admin", false).await;
    seed_user(&pool, "operator1", "op123", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "operator1", "op123");
    let req = add_cookies(
        test::TestRequest::delete()
            .uri(&format!("/api/users/{uid}"))
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF)),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "users.delete");
}

// ─── VAL-RBAC-011: Viewer cannot manage users → 403 ───────────────────

#[actix_web::test]
async fn viewer_cannot_list_users() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "viewer1", "view123");
    let req = add_cookies(test::TestRequest::get().uri("/api/users"), &cookies).to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "users.list");
}

#[actix_web::test]
async fn viewer_cannot_create_user() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "viewer1", "view123");
    let req = add_cookies(
        test::TestRequest::post()
            .uri("/api/users")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(serde_json::json!({
                "username": "newuser",
                "password": "pass123",
                "role": "viewer"
            })),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "users.create");
}

#[actix_web::test]
async fn viewer_cannot_get_user() {
    let pool = test_pool().await;
    let uid = seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "viewer1", "view123");
    let req = add_cookies(
        test::TestRequest::get().uri(&format!("/api/users/{uid}")),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "users.read");
}

#[actix_web::test]
async fn viewer_cannot_patch_user() {
    let pool = test_pool().await;
    let uid = seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "viewer1", "view123");
    let req = add_cookies(
        test::TestRequest::patch()
            .uri(&format!("/api/users/{uid}"))
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(serde_json::json!({ "displayName": "hacked" })),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "users.update");
}

#[actix_web::test]
async fn viewer_cannot_delete_user() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "admin2", "admin2123", "admin", false).await;
    let uid = seed_user(&pool, "bob", "bob123", "operator", false).await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "viewer1", "view123");
    let req = add_cookies(
        test::TestRequest::delete()
            .uri(&format!("/api/users/{uid}"))
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF)),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "users.delete");
}

// ─── VAL-RBAC-012: Operator cannot activate a license → 403 ───────────

#[actix_web::test]
async fn operator_cannot_activate_license() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "operator1", "op123", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "operator1", "op123");
    let code = make_valid_code(10);
    let req = add_cookies(
        test::TestRequest::post()
            .uri("/api/license/activate")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(serde_json::json!({ "code": code })),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "license.activate");
}

// ─── VAL-RBAC-013: Viewer cannot activate a license → 403 ─────────────

#[actix_web::test]
async fn viewer_cannot_activate_license() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "viewer1", "view123");
    let code = make_valid_code(10);
    let req = add_cookies(
        test::TestRequest::post()
            .uri("/api/license/activate")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(serde_json::json!({ "code": code })),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "license.activate");
}

// ─── VAL-RBAC-015: Server-side enforcement (curl bypass) → 403 ────────
// A viewer with a valid session + CSRF token cannot bypass RBAC by calling
// an admin-only endpoint directly. This is the same as the viewer tests above
// but explicitly framed as "server-side enforcement regardless of client-side
// gating" per the contract.

#[actix_web::test]
async fn server_side_enforcement_viewer_cannot_create_user() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    // Viewer with valid cookie + CSRF token (bypassing SPA hidden buttons)
    let cookies = do_login!(app, "viewer1", "view123");
    let req = add_cookies(
        test::TestRequest::post()
            .uri("/api/users")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(serde_json::json!({
                "username": "sneaky",
                "password": "pass123",
                "role": "admin"
            })),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "users.create");

    // Verify no user was created
    let users = dt_console_server::repositories::user_repository::UserRepository::list(&pool)
        .await
        .unwrap();
    assert_eq!(
        users.len(),
        2,
        "only admin and viewer should exist, no sneaky user"
    );
}

// ─── VAL-RBAC-001: Admin can perform every action ─────────────────────

#[actix_web::test]
async fn admin_can_list_users() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "admin", "admin123");
    let req = add_cookies(test::TestRequest::get().uri("/api/users"), &cookies).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[actix_web::test]
async fn admin_can_create_user() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "admin", "admin123");
    let req = add_cookies(
        test::TestRequest::post()
            .uri("/api/users")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(serde_json::json!({
                "username": "newuser",
                "password": "pass123",
                "role": "viewer"
            })),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::CREATED);
}

#[actix_web::test]
async fn admin_can_activate_license() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "admin", "admin123");
    let code = make_valid_code(10);
    let req = add_cookies(
        test::TestRequest::post()
            .uri("/api/license/activate")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(serde_json::json!({ "code": code })),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[actix_web::test]
async fn admin_can_delete_user() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "admin2", "admin2123", "admin", false).await;
    let uid = seed_user(&pool, "bob", "bob123", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "admin", "admin123");
    let req = add_cookies(
        test::TestRequest::delete()
            .uri(&format!("/api/users/{uid}"))
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF)),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}

// ─── VAL-RBAC-003: All roles can read license (GET /api/license) ──────

#[actix_web::test]
async fn all_roles_can_get_license() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "operator1", "op123", "operator", false).await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    for (username, password) in [
        ("admin", "admin123"),
        ("operator1", "op123"),
        ("viewer1", "view123"),
    ] {
        let cookies = do_login!(app, username, password);
        let req = add_cookies(test::TestRequest::get().uri("/api/license"), &cookies).to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "{username} should be able to GET /api/license"
        );
    }
}

// ─── Operate logs: operator/viewer → 403 ──────────────────────────────

#[actix_web::test]
async fn operator_cannot_read_operate_logs() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "operator1", "op123", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "operator1", "op123");
    let req = add_cookies(test::TestRequest::get().uri("/api/operate_logs"), &cookies).to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "operate_logs.list");
}

#[actix_web::test]
async fn viewer_cannot_read_operate_logs() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "viewer1", "view123");
    let req = add_cookies(test::TestRequest::get().uri("/api/operate_logs"), &cookies).to_request();
    let res = test::call_service(&app, req).await;
    assert_forbidden!(res, "operate_logs.list");
}

#[actix_web::test]
async fn admin_can_read_operate_logs() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let cookies = do_login!(app, "admin", "admin123");
    let req = add_cookies(test::TestRequest::get().uri("/api/operate_logs"), &cookies).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
}

// ─── VAL-SEC-INJ-001: SQL injection on filter endpoints prevented ─────

#[actix_web::test]
async fn operate_logs_filter_sql_injection_safe() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    let cookies = do_login!(app, "admin", "admin123");

    // SQL injection attempt in actor filter — URL-encode the injection payload
    // ');DROP TABLE operate_logs;-- → %27%29%3BDROP%20TABLE%20operate_logs%3B--
    let req = add_cookies(
        test::TestRequest::get()
            .uri("/api/operate_logs?actor=%27%29%3BDROP%20TABLE%20operate_logs%3B--"),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    // Should return 200 with empty page (parameterised query neutralises injection)
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["total"], 0);

    // Verify table still exists and has rows
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM operate_logs")
        .fetch_one(&pool)
        .await
        .unwrap();
    // Table should not have been dropped
    assert!(count >= 0, "operate_logs table should still exist");
}

#[actix_web::test]
async fn operate_logs_filter_boolean_blind_safe() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    let cookies = do_login!(app, "admin", "admin123");

    // Boolean-blind injection attempt
    let req = add_cookies(
        test::TestRequest::get().uri("/api/operate_logs?actor=%27%20OR%20%271%27%3D%271"),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    // Should return 0 matches (parameterised query treats the entire string as a literal value)
    assert_eq!(
        body["total"], 0,
        "boolean-blind injection should not leak data"
    );
}

// ─── Table-driven RBAC integration test ────────────────────────────────
// This tests the full (role, endpoint, method, expected_status) matrix
// for all endpoints that currently exist.

#[allow(clippy::type_complexity)]
#[actix_web::test]
async fn rbac_endpoint_matrix_existing_endpoints() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "admin2", "admin2123", "admin", false).await;
    let target_uid = seed_user(&pool, "target", "target123", "operator", false).await;
    seed_user(&pool, "operator1", "op123", "operator", false).await;
    seed_user(&pool, "viewer1", "view123", "viewer", false).await;

    let app = test::init_service(build_test_app(pool)).await;

    // Pre-compute format strings to avoid temporary value issues
    let get_user_path = format!("/api/users/{target_uid}");

    // (username, password, method, path, csrf_needed, body, expected_status)
    let cases: Vec<(
        &str,
        &str,
        &str,
        &str,
        bool,
        Option<serde_json::Value>,
        StatusCode,
    )> = vec![
        // Admin can do everything
        (
            "admin",
            "admin123",
            "GET",
            "/api/users",
            false,
            None,
            StatusCode::OK,
        ),
        (
            "admin",
            "admin123",
            "POST",
            "/api/users",
            true,
            Some(serde_json::json!({"username":"x1","password":"p","role":"viewer"})),
            StatusCode::CREATED,
        ),
        (
            "admin",
            "admin123",
            "GET",
            &get_user_path,
            false,
            None,
            StatusCode::OK,
        ),
        (
            "admin",
            "admin123",
            "PATCH",
            &get_user_path,
            true,
            Some(serde_json::json!({"displayName":"patched"})),
            StatusCode::OK,
        ),
        (
            "admin",
            "admin123",
            "GET",
            "/api/license",
            false,
            None,
            StatusCode::OK,
        ),
        (
            "admin",
            "admin123",
            "POST",
            "/api/license/activate",
            true,
            Some(serde_json::json!({"code": make_valid_code(10)})),
            StatusCode::OK,
        ),
        (
            "admin",
            "admin123",
            "GET",
            "/api/operate_logs",
            false,
            None,
            StatusCode::OK,
        ),
        // Operator: users → 403
        (
            "operator1",
            "op123",
            "GET",
            "/api/users",
            false,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "operator1",
            "op123",
            "POST",
            "/api/users",
            true,
            Some(serde_json::json!({"username":"x2","password":"p","role":"viewer"})),
            StatusCode::FORBIDDEN,
        ),
        (
            "operator1",
            "op123",
            "GET",
            &get_user_path,
            false,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "operator1",
            "op123",
            "PATCH",
            &get_user_path,
            true,
            Some(serde_json::json!({"displayName":"patched"})),
            StatusCode::FORBIDDEN,
        ),
        // Operator: license read → 200, activate → 403
        (
            "operator1",
            "op123",
            "GET",
            "/api/license",
            false,
            None,
            StatusCode::OK,
        ),
        (
            "operator1",
            "op123",
            "POST",
            "/api/license/activate",
            true,
            Some(serde_json::json!({"code": make_valid_code(10)})),
            StatusCode::FORBIDDEN,
        ),
        // Operator: operate_logs → 403
        (
            "operator1",
            "op123",
            "GET",
            "/api/operate_logs",
            false,
            None,
            StatusCode::FORBIDDEN,
        ),
        // Viewer: users → 403
        (
            "viewer1",
            "view123",
            "GET",
            "/api/users",
            false,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "viewer1",
            "view123",
            "POST",
            "/api/users",
            true,
            Some(serde_json::json!({"username":"x3","password":"p","role":"viewer"})),
            StatusCode::FORBIDDEN,
        ),
        (
            "viewer1",
            "view123",
            "GET",
            &get_user_path,
            false,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "viewer1",
            "view123",
            "PATCH",
            &get_user_path,
            true,
            Some(serde_json::json!({"displayName":"patched"})),
            StatusCode::FORBIDDEN,
        ),
        // Viewer: license read → 200, activate → 403
        (
            "viewer1",
            "view123",
            "GET",
            "/api/license",
            false,
            None,
            StatusCode::OK,
        ),
        (
            "viewer1",
            "view123",
            "POST",
            "/api/license/activate",
            true,
            Some(serde_json::json!({"code": make_valid_code(10)})),
            StatusCode::FORBIDDEN,
        ),
        // Viewer: operate_logs → 403
        (
            "viewer1",
            "view123",
            "GET",
            "/api/operate_logs",
            false,
            None,
            StatusCode::FORBIDDEN,
        ),
    ];

    for (username, password, method, path, csrf, body, expected) in &cases {
        let cookies = {
            let req = test::TestRequest::post()
                .uri("/api/auth/login")
                .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
                .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
                .insert_header((XSRF_HEADER_NAME, XSRF))
                .set_json(LoginRequest {
                    username: username.to_string(),
                    password: password.to_string(),
                })
                .to_request();
            let res = test::call_service(&app, req).await;
            collect_cookies(&res)
        };

        let mut req = match *method {
            "GET" => test::TestRequest::get().uri(path),
            "POST" => test::TestRequest::post().uri(path),
            "PATCH" => test::TestRequest::patch().uri(path),
            "DELETE" => test::TestRequest::delete().uri(path),
            _ => panic!("unsupported method: {method}"),
        };

        if *csrf && body.is_some() {
            req = req
                .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
                .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
                .insert_header((XSRF_HEADER_NAME, XSRF));
        }
        if let Some(b) = body.clone() {
            req = req.set_json(b);
        }
        req = add_cookies(req, &cookies);

        let res = test::call_service(&app, req.to_request()).await;
        assert_eq!(
            res.status(),
            *expected,
            "{username} {method} {path} expected {} got {}",
            *expected,
            res.status()
        );
    }
}
