//! Integration tests for user management endpoints:
//! GET/POST/PATCH/DELETE /api/users[/:id]
//!
//! Covers:
//! - VAL-USER-001: Admin creates a user → 201
//! - VAL-USER-002: Duplicate username → 409
//! - VAL-USER-003: Stored password hash uses bcrypt with cost ≥ 10
//! - VAL-USER-004: Cleartext password is never persisted
//! - VAL-USER-005: Admin password reset takes effect immediately
//! - VAL-USER-006: Disabled account cannot log in or use existing session
//! - VAL-USER-007: Cannot delete the last admin → 409
//! - VAL-USER-008: Deleted user cannot login
//! - VAL-SEC-USER-001: Users cannot promote themselves via self-PATCH
//! - RBAC: operator/viewer cannot manage users
//! - Password reset invalidates sessions
//! - Disable user invalidates sessions
//! - User deletion cascades sessions safely

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
use dt_console_server::log_sse_handlers;
use dt_console_server::metrics_scraper;
use dt_console_server::middleware::csrf::{Csrf, XSRF_COOKIE_NAME, XSRF_HEADER_NAME};
use dt_console_server::models::LoginRequest;
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use dt_console_server::user_handlers;
use sqlx::SqlitePool;

// ─── Helpers ─────────────────────────────────────────────────────────────

/// Create a migrated test pool backed by a temp file.
async fn test_pool() -> SqlitePool {
    let dir = std::env::temp_dir().join("dt-console-server-user-test");
    std::fs::create_dir_all(&dir).unwrap();
    let test_name = std::thread::current()
        .name()
        .unwrap_or("unknown")
        .to_string();
    let safe_name: String = test_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let path = dir.join(format!("user-{safe_name}.db"));
    let _ = std::fs::remove_file(&path);
    let path_str = path.to_string_lossy().to_string();
    let pool = db::create_pool(&path_str).await.unwrap();
    db::run_migrations(&pool).await.unwrap();
    pool
}

const IDLE_TIMEOUT_SECS: i64 = 3600;
const XSRF: &str = "test-xsrf-token";

/// Build the standard test app with full middleware stack including user handlers.
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
                .service(user_handlers::delete_user),
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

/// Add a set of cookies to a TestRequest.
fn add_cookies(mut req: test::TestRequest, cookies: &[Cookie<'static>]) -> test::TestRequest {
    for c in cookies {
        req = req.cookie(c.clone());
    }
    req
}

/// Login as a user and return the session cookies.
/// Uses a macro because the app type from `test::init_service` is unnameable.
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

/// Build a POST /api/users request with auth cookies and CSRF.
fn post_user_req(cookies: &[Cookie<'static>], body: serde_json::Value) -> test::TestRequest {
    add_cookies(
        test::TestRequest::post()
            .uri("/api/users")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF)),
        cookies,
    )
    .set_json(body)
}

/// Build a GET /api/users request with auth cookies.
fn list_users_req(cookies: &[Cookie<'static>]) -> test::TestRequest {
    add_cookies(test::TestRequest::get().uri("/api/users"), cookies)
}

/// Build a GET /api/users/:id request with auth cookies.
fn get_user_req(cookies: &[Cookie<'static>], id: &str) -> test::TestRequest {
    add_cookies(
        test::TestRequest::get().uri(&format!("/api/users/{id}")),
        cookies,
    )
}

/// Build a PATCH /api/users/:id request with auth cookies and CSRF.
fn patch_user_req(
    cookies: &[Cookie<'static>],
    id: &str,
    body: serde_json::Value,
) -> test::TestRequest {
    add_cookies(
        test::TestRequest::patch()
            .uri(&format!("/api/users/{id}"))
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF)),
        cookies,
    )
    .set_json(body)
}

/// Build a DELETE /api/users/:id request with auth cookies and CSRF.
fn delete_user_req(cookies: &[Cookie<'static>], id: &str) -> test::TestRequest {
    add_cookies(
        test::TestRequest::delete()
            .uri(&format!("/api/users/{id}"))
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF)),
        cookies,
    )
}

/// Build a POST /api/auth/login request.
fn login_req(username: &str, password: &str) -> test::TestRequest {
    test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: username.to_string(),
            password: password.to_string(),
        })
}

/// Build a GET /api/auth/me request with cookies.
fn me_req(cookies: &[Cookie<'static>]) -> test::TestRequest {
    add_cookies(test::TestRequest::get().uri("/api/auth/me"), cookies)
}

// ─── VAL-USER-001: Admin creates a user ─────────────────────────────────

#[actix_web::test]
async fn admin_create_user_returns_201() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(&app, "admin", "admin123");

    let req = post_user_req(
        &cookies,
        serde_json::json!({
            "username": "alice",
            "password": "alicepass1",
            "role": "operator",
            "displayName": "Alice"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["username"], "alice");
    assert_eq!(body["role"], "operator");
    assert_eq!(body["displayName"], "Alice");
    assert_eq!(body["disabled"], false);
    assert!(body.get("id").is_some(), "response must include id");
    assert!(
        body.get("createdAt").is_some(),
        "response must include createdAt"
    );
    // Password must NEVER appear in response
    assert!(body.get("password").is_none(), "password must not appear");
    assert!(
        body.get("passwordHash").is_none(),
        "password_hash must not appear"
    );
    assert!(
        body.get("password_hash").is_none(),
        "password_hash must not appear"
    );
    assert!(body.get("salt").is_none(), "salt must not appear");
}

// ─── VAL-USER-002: Duplicate username is rejected ────────────────────────

#[actix_web::test]
async fn create_user_duplicate_username_returns_409() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "alice", "alicepass1", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(&app, "admin", "admin123");

    let req = post_user_req(
        &cookies,
        serde_json::json!({
            "username": "alice",
            "password": "differentpw",
            "role": "viewer"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::USERNAME_TAKEN);
}

// ─── VAL-USER-003: Stored password hash uses bcrypt with cost ≥ 10 ──────

#[actix_web::test]
async fn created_user_has_bcrypt_hash_cost_at_least_10() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let pool_clone = pool.clone();
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(&app, "admin", "admin123");

    let req = post_user_req(
        &cookies,
        serde_json::json!({
            "username": "bob",
            "password": "bobpass1",
            "role": "viewer"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(res).await;
    let bob_id = body["id"].as_str().unwrap();

    // Directly check the DB for the password hash using the cloned pool
    let bob = dt_console_server::repositories::user_repository::UserRepository::find_by_id(
        &pool_clone,
        bob_id,
    )
    .await
    .unwrap();

    // bcrypt hash format: $2b$10$...
    assert!(
        bob.password_hash.starts_with("$2b$") || bob.password_hash.starts_with("$2a$"),
        "hash should start with $2b$ or $2a$, got: {}",
        &bob.password_hash[..8]
    );
    let parts: Vec<&str> = bob.password_hash.split('$').collect();
    let cost: u32 = parts[2].parse().unwrap();
    assert!(cost >= 10, "bcrypt cost must be ≥ 10, got {cost}");

    // Verify the hash matches the known password
    assert!(bcrypt::verify("bobpass1", &bob.password_hash).unwrap());
}

// ─── VAL-USER-004: Cleartext password is never persisted ────────────────

#[actix_web::test]
async fn cleartext_password_not_in_db_or_response() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let pool_clone = pool.clone();
    let app = test::init_service(build_test_app(pool)).await;
    let cookies = do_login!(&app, "admin", "admin123");

    let plaintext = "verysecret123";
    let req = post_user_req(
        &cookies,
        serde_json::json!({
            "username": "carol",
            "password": plaintext,
            "role": "operator"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // Response body must not contain cleartext
    let body_bytes = test::read_body(res).await;
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    assert!(
        !body_str.contains(plaintext),
        "cleartext password must not appear in response body"
    );

    // DB columns must not contain cleartext
    let users = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT id, username, password_hash, display_name, role FROM users WHERE username = 'carol'"
    )
    .fetch_one(&pool_clone)
    .await
    .unwrap();

    let (_, _, hash, _, _) = users;
    assert!(
        !hash.contains(plaintext),
        "cleartext password must not appear in password_hash column"
    );
}

// ─── VAL-USER-005: Admin password reset takes effect immediately ────────

#[actix_web::test]
async fn password_reset_takes_effect_immediately() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let user_id = seed_user(&pool, "dave", "oldpass123", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    // First, login as dave with old password to get a session
    let dave_cookies = do_login!(&app, "dave", "oldpass123");

    // Admin resets dave's password
    let req = patch_user_req(
        &admin_cookies,
        &user_id,
        serde_json::json!({
            "password": "newpw456"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Old session should be invalidated
    let me_res = test::call_service(&app, me_req(&dave_cookies).to_request()).await;
    assert_eq!(
        me_res.status(),
        StatusCode::UNAUTHORIZED,
        "old session must be invalidated after password reset"
    );

    // Login with old password should fail
    let old_res = test::call_service(&app, login_req("dave", "oldpass123").to_request()).await;
    assert_eq!(old_res.status(), StatusCode::UNAUTHORIZED);

    // Login with new password should succeed
    let new_res = test::call_service(&app, login_req("dave", "newpw456").to_request()).await;
    assert_eq!(new_res.status(), StatusCode::OK);
}

// ─── VAL-USER-006: Disabled account cannot login or use existing session ─

#[actix_web::test]
async fn disable_user_rejects_existing_session_and_new_login() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let user_id = seed_user(&pool, "eve", "evepass1", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    // Login as eve first
    let eve_cookies = do_login!(&app, "eve", "evepass1");

    // Admin disables eve
    let req = patch_user_req(
        &admin_cookies,
        &user_id,
        serde_json::json!({
            "disabled": true
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Eve's existing session should be rejected
    let me_res = test::call_service(&app, me_req(&eve_cookies).to_request()).await;
    assert_eq!(me_res.status(), StatusCode::UNAUTHORIZED);
    let me_body: serde_json::Value = test::read_body_json(me_res).await;
    assert_eq!(me_body["code"], error::codes::ACCOUNT_DISABLED);

    // New login with correct credentials should also fail
    let login_res = test::call_service(&app, login_req("eve", "evepass1").to_request()).await;
    assert_eq!(login_res.status(), StatusCode::UNAUTHORIZED);
    let login_body: serde_json::Value = test::read_body_json(login_res).await;
    assert_eq!(login_body["code"], error::codes::ACCOUNT_DISABLED);
}

// ─── VAL-USER-007: Cannot delete the last admin ─────────────────────────

#[actix_web::test]
async fn cannot_delete_last_admin_returns_409() {
    let pool = test_pool().await;
    let admin_id = seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    let req = delete_user_req(&admin_cookies, &admin_id).to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::LAST_ADMIN_PROTECTED);
}

// ─── VAL-USER-008: Deleted user cannot login ────────────────────────────

#[actix_web::test]
async fn deleted_user_cannot_login() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let bob_id = seed_user(&pool, "bob", "bobpass1", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    // Delete bob
    let req = delete_user_req(&admin_cookies, &bob_id).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    // Bob cannot login
    let login_res = test::call_service(&app, login_req("bob", "bobpass1").to_request()).await;
    assert_eq!(login_res.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(login_res).await;
    assert_eq!(body["code"], error::codes::INVALID_CREDENTIALS);
}

// ─── VAL-SEC-USER-001: Self-PATCH cannot promote own role ──────────────

#[actix_web::test]
async fn self_patch_cannot_promote_own_role_operator() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let op_id = seed_user(&pool, "operator1", "oppass1", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let op_cookies = do_login!(&app, "operator1", "oppass1");

    // Operator tries to PATCH themselves to admin — but operators can't use user endpoints at all
    let req = patch_user_req(
        &op_cookies,
        &op_id,
        serde_json::json!({
            "role": "admin"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::FORBIDDEN);
}

#[actix_web::test]
async fn admin_self_patch_cannot_demote_self() {
    let pool = test_pool().await;
    let admin_id = seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    // Admin tries to demote themselves to viewer
    let req = patch_user_req(
        &admin_cookies,
        &admin_id,
        serde_json::json!({
            "role": "viewer"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::CANNOT_DEMOTE_SELF);
}

// ─── RBAC: Operator cannot manage users ─────────────────────────────────

#[actix_web::test]
async fn operator_cannot_create_user() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "operator1", "oppass1", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let op_cookies = do_login!(&app, "operator1", "oppass1");

    let req = post_user_req(
        &op_cookies,
        serde_json::json!({
            "username": "newuser",
            "password": "pass123",
            "role": "viewer"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::FORBIDDEN);
}

#[actix_web::test]
async fn operator_cannot_list_users() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "operator1", "oppass1", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let op_cookies = do_login!(&app, "operator1", "oppass1");

    let req = list_users_req(&op_cookies).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[actix_web::test]
async fn operator_cannot_patch_user() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let target_id = seed_user(&pool, "viewer1", "vpass1", "viewer", false).await;
    seed_user(&pool, "operator1", "oppass1", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let op_cookies = do_login!(&app, "operator1", "oppass1");

    let req = patch_user_req(
        &op_cookies,
        &target_id,
        serde_json::json!({ "displayName": "Hacked" }),
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[actix_web::test]
async fn operator_cannot_delete_user() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let target_id = seed_user(&pool, "viewer1", "vpass1", "viewer", false).await;
    seed_user(&pool, "operator1", "oppass1", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let op_cookies = do_login!(&app, "operator1", "oppass1");

    let req = delete_user_req(&op_cookies, &target_id).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

// ─── RBAC: Viewer cannot manage users ────────────────────────────────────

#[actix_web::test]
async fn viewer_cannot_list_users() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "viewer1", "vpass1", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let viewer_cookies = do_login!(&app, "viewer1", "vpass1");

    let req = list_users_req(&viewer_cookies).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[actix_web::test]
async fn viewer_cannot_create_user() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "viewer1", "vpass1", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let viewer_cookies = do_login!(&app, "viewer1", "vpass1");

    let req = post_user_req(
        &viewer_cookies,
        serde_json::json!({
            "username": "newuser",
            "password": "pass123",
            "role": "viewer"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

// ─── User deletion cascades sessions safely ─────────────────────────────

#[actix_web::test]
async fn user_deletion_cascades_sessions() {
    let pool = test_pool().await;
    seed_user(&pool, "admin2", "admin123", "admin", false).await;
    let bob_id = seed_user(&pool, "bob", "bobpass1", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    // Bob logs in
    let bob_cookies = do_login!(&app, "bob", "bobpass1");

    // Verify bob's session works
    let me_res = test::call_service(&app, me_req(&bob_cookies).to_request()).await;
    assert_eq!(me_res.status(), StatusCode::OK);

    // Admin deletes bob
    let admin_cookies = do_login!(&app, "admin2", "admin123");
    let del_req = delete_user_req(&admin_cookies, &bob_id).to_request();
    let del_res = test::call_service(&app, del_req).await;
    assert_eq!(del_res.status(), StatusCode::NO_CONTENT);

    // Bob's old session cookie should be rejected
    let me_res2 = test::call_service(&app, me_req(&bob_cookies).to_request()).await;
    assert_eq!(me_res2.status(), StatusCode::UNAUTHORIZED);
}

// ─── GET /api/users/:id — admin can get a single user ───────────────────

#[actix_web::test]
async fn admin_get_single_user() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let user_id = seed_user(&pool, "frank", "frankpass", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    let req = get_user_req(&admin_cookies, &user_id).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["id"], user_id);
    assert_eq!(body["username"], "frank");
    assert_eq!(body["role"], "viewer");
    assert!(body.get("password").is_none());
    assert!(body.get("passwordHash").is_none());
}

// ─── GET /api/users/:id — non-existent user returns 404 ─────────────────

#[actix_web::test]
async fn get_nonexistent_user_returns_404() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    let req = get_user_req(&admin_cookies, "00000000-0000-0000-0000-000000000000").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::NOT_FOUND);
}

// ─── PATCH /api/users/:id — update display name ─────────────────────────

#[actix_web::test]
async fn admin_update_user_display_name() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let user_id = seed_user(&pool, "grace", "gracepass", "operator", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    let req = patch_user_req(
        &admin_cookies,
        &user_id,
        serde_json::json!({
            "displayName": "Grace Hopper"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["displayName"], "Grace Hopper");
    assert_eq!(body["username"], "grace"); // unchanged
}

// ─── PATCH /api/users/:id — update role ─────────────────────────────────

#[actix_web::test]
async fn admin_can_change_another_users_role() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let user_id = seed_user(&pool, "heidi", "heidipass", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    let req = patch_user_req(
        &admin_cookies,
        &user_id,
        serde_json::json!({
            "role": "operator"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["role"], "operator");
}

// ─── Can delete admin when there are multiple admins ─────────────────────

#[actix_web::test]
async fn can_delete_admin_when_multiple_exist() {
    let pool = test_pool().await;
    let admin1_id = seed_user(&pool, "admin1", "admin123", "admin", false).await;
    seed_user(&pool, "admin2", "admin456", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin2_cookies = do_login!(&app, "admin2", "admin456");

    let req = delete_user_req(&admin2_cookies, &admin1_id).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    // Deleted admin1 cannot login
    let login_res = test::call_service(&app, login_req("admin1", "admin123").to_request()).await;
    assert_eq!(login_res.status(), StatusCode::UNAUTHORIZED);
}

// ─── Re-enable a disabled user ──────────────────────────────────────────

#[actix_web::test]
async fn admin_can_reenable_disabled_user() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let user_id = seed_user(&pool, "irene", "irenepass", "viewer", true).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    // Enable the user
    let req = patch_user_req(
        &admin_cookies,
        &user_id,
        serde_json::json!({
            "disabled": false
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["disabled"], false);

    // Now the user can login
    let login_res = test::call_service(&app, login_req("irene", "irenepass").to_request()).await;
    assert_eq!(login_res.status(), StatusCode::OK);
}

// ─── List users returns all users ────────────────────────────────────────

#[actix_web::test]
async fn admin_list_users_returns_all() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "user1", "pass1", "operator", false).await;
    seed_user(&pool, "user2", "pass2", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    let req = list_users_req(&admin_cookies).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: Vec<serde_json::Value> = test::read_body_json(res).await;
    assert_eq!(body.len(), 3, "should list all 3 users");

    // Verify no password fields leak
    for user in &body {
        assert!(user.get("password").is_none());
        assert!(user.get("passwordHash").is_none());
        assert!(user.get("password_hash").is_none());
    }
}

// ─── Anonymous requests to user endpoints return 401 ─────────────────────

#[actix_web::test]
async fn anonymous_get_users_returns_401() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::get().uri("/api/users").to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn anonymous_create_user_returns_401() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::post()
        .uri("/api/users")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(serde_json::json!({
            "username": "sneaky",
            "password": "pass",
            "role": "admin"
        }))
        .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

// ─── Admin self-PATCH with same role is allowed ─────────────────────────

#[actix_web::test]
async fn admin_self_patch_same_role_allowed() {
    let pool = test_pool().await;
    let admin_id = seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    // Admin PATCHes themselves with role=admin (same as current) — should succeed
    let req = patch_user_req(
        &admin_cookies,
        &admin_id,
        serde_json::json!({
            "role": "admin",
            "displayName": "Super Admin"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["role"], "admin");
    assert_eq!(body["displayName"], "Super Admin");
}

// ─── Validate role values on create ──────────────────────────────────────

#[actix_web::test]
async fn create_user_invalid_role_returns_400() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    let req = post_user_req(
        &admin_cookies,
        serde_json::json!({
            "username": "badrole",
            "password": "pass",
            "role": "superadmin"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::VALIDATION_FAILED);
}

// ─── Validate role values on update ─────────────────────────────────────

#[actix_web::test]
async fn update_user_invalid_role_returns_400() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let user_id = seed_user(&pool, "testuser", "pass", "viewer", false).await;
    let app = test::init_service(build_test_app(pool)).await;
    let admin_cookies = do_login!(&app, "admin", "admin123");

    let req = patch_user_req(
        &admin_cookies,
        &user_id,
        serde_json::json!({
            "role": "superadmin"
        }),
    )
    .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::VALIDATION_FAILED);
}
