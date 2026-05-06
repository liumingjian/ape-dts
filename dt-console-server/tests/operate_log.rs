//! Integration tests for the operate_log audit system.
//!
//! Covers:
//! - VAL-AUDIT-001: Successful login writes one operate_log row
//! - VAL-AUDIT-002: Failed login writes one operate_log row
//! - VAL-AUDIT-003: Each state mutation writes exactly one operate_log row
//! - VAL-AUDIT-004: Admin can read /api/operate_logs
//! - VAL-AUDIT-005: Operator and viewer cannot read /api/operate_logs
//! - VAL-AUDIT-006: Operate log rows are immutable and activation codes are redacted
//! - VAL-SEC-LEAK-001: Sensitive fields are redacted across every log/response surface

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
use dt_console_server::middleware::csrf::{Csrf, XSRF_COOKIE_NAME, XSRF_HEADER_NAME};
use dt_console_server::models::LoginRequest;
use dt_console_server::operate_log_handlers;
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use dt_console_server::repositories::operate_log_repository::OperateLogRepository;
use dt_console_server::user_handlers;
use sqlx::SqlitePool;

// ─── Helpers ─────────────────────────────────────────────────────────────

async fn test_pool() -> SqlitePool {
    let dir = std::env::temp_dir().join("dt-console-server-oplog-test");
    std::fs::create_dir_all(&dir).unwrap();
    let test_name = std::thread::current()
        .name()
        .unwrap_or("unknown")
        .to_string();
    let safe_name: String = test_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let path = dir.join(format!("oplog-{safe_name}.db"));
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
                .service(operate_log_handlers::list_operate_logs),
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

/// Login expecting failure.
macro_rules! do_login_fail {
    ($app:expr, $username:expr, $password:expr, $expected_status:expr) => {{
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
        assert_eq!(res.status(), $expected_status);
        collect_cookies(&res)
    }};
}

/// Count operate_log rows in the database.
async fn count_operate_logs(pool: &SqlitePool) -> i64 {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM operate_logs")
        .fetch_one(pool)
        .await
        .unwrap();
    row.0
}

/// Count operate_log rows matching a specific action and result.
async fn count_operate_logs_by(pool: &SqlitePool, action: &str, result: &str) -> i64 {
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM operate_logs WHERE action = ? AND result = ?")
            .bind(action)
            .bind(result)
            .fetch_one(pool)
            .await
            .unwrap();
    row.0
}

/// Build a GET /api/operate_logs request with auth cookies.
fn get_operate_logs(cookies: &[Cookie<'static>], query: &str) -> test::TestRequest {
    add_cookies(
        test::TestRequest::get().uri(&format!("/api/operate_logs{query}")),
        cookies,
    )
}

/// Build a POST /api/users request with auth cookies and CSRF.
fn post_user(cookies: &[Cookie<'static>], body: serde_json::Value) -> test::TestRequest {
    add_cookies(
        test::TestRequest::post()
            .uri("/api/users")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF)),
        cookies,
    )
    .set_json(body)
}

/// Build a PATCH /api/users/:id request with auth cookies and CSRF.
fn patch_user(cookies: &[Cookie<'static>], id: &str, body: serde_json::Value) -> test::TestRequest {
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
fn delete_user(cookies: &[Cookie<'static>], id: &str) -> test::TestRequest {
    add_cookies(
        test::TestRequest::delete()
            .uri(&format!("/api/users/{id}"))
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF)),
        cookies,
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────

/// VAL-AUDIT-001: Successful login writes exactly one operate_log row.
#[actix_web::test]
async fn successful_login_writes_one_operate_log() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let before = count_operate_logs(&pool).await;
    let _cookies = do_login!(app, "admin", "admin123");
    let after = count_operate_logs(&pool).await;

    assert_eq!(
        after - before,
        1,
        "successful login should write exactly one operate_log row"
    );

    // Verify the row content
    let logs = OperateLogRepository::list(&pool).await.unwrap();
    let login_log = logs.iter().find(|l| l.action == "auth.login").unwrap();
    assert_eq!(login_log.actor, "admin");
    assert_eq!(login_log.result, "success");
    assert!(
        login_log.details.is_none() || !login_log.details.as_ref().unwrap().contains("admin123"),
        "operate_log must not contain password"
    );
}

/// VAL-AUDIT-002: Failed login writes one operate_log row with result=failure.
#[actix_web::test]
async fn failed_login_writes_one_operate_log_row() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let before = count_operate_logs(&pool).await;
    let _cookies = do_login_fail!(app, "admin", "wrongpw", StatusCode::UNAUTHORIZED);
    let after = count_operate_logs(&pool).await;

    assert_eq!(
        after - before,
        1,
        "failed login should write exactly one operate_log row"
    );

    let logs = OperateLogRepository::list(&pool).await.unwrap();
    let login_log = logs
        .iter()
        .find(|l| l.action == "auth.login" && l.result == "failure")
        .unwrap();
    assert_eq!(login_log.actor, "admin");
    assert_eq!(login_log.result, "failure");
    // Details should contain reason but not the password
    if let Some(ref details) = login_log.details {
        assert!(
            !details.contains("wrongpw"),
            "details must not contain the attempted password"
        );
    }
}

/// VAL-AUDIT-002: Unknown username login writes one operate_log row.
#[actix_web::test]
async fn unknown_username_login_writes_operate_log() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let before = count_operate_logs(&pool).await;
    let _cookies = do_login_fail!(app, "nonexistent", "anypw", StatusCode::UNAUTHORIZED);
    let after = count_operate_logs(&pool).await;

    assert_eq!(
        after - before,
        1,
        "unknown username login should write one operate_log row"
    );

    let logs = OperateLogRepository::list(&pool).await.unwrap();
    let login_log = logs
        .iter()
        .find(|l| l.action == "auth.login" && l.result == "failure")
        .unwrap();
    assert_eq!(login_log.actor, "nonexistent");
}

/// VAL-AUDIT-002: Disabled account login writes operate_log with result=failure.
#[actix_web::test]
async fn disabled_account_login_writes_operate_log() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "disabled_user", "pass123", "operator", true).await;

    let before = count_operate_logs(&pool).await;
    let _cookies = do_login_fail!(app, "disabled_user", "pass123", StatusCode::UNAUTHORIZED);
    let after = count_operate_logs(&pool).await;

    assert_eq!(
        after - before,
        1,
        "disabled account login should write one operate_log row"
    );
}

/// VAL-AUDIT-003: User create writes one operate_log row.
#[actix_web::test]
async fn user_create_writes_one_operate_log() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    let before = count_operate_logs_by(&pool, "users.create", "success").await;

    let req = post_user(
        &cookies,
        serde_json::json!({
            "username": "newuser",
            "password": "newpass123",
            "role": "viewer",
            "displayName": "New User"
        }),
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::CREATED);

    let after = count_operate_logs_by(&pool, "users.create", "success").await;
    assert_eq!(
        after - before,
        1,
        "user create should write exactly one operate_log row"
    );
}

/// VAL-AUDIT-003: User update writes one operate_log row.
#[actix_web::test]
async fn user_update_writes_one_operate_log() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    let _admin_id = seed_user(&pool, "admin", "admin123", "admin", false).await;
    let user_id = seed_user(&pool, "bob", "bobpass", "operator", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    let before = count_operate_logs_by(&pool, "users.update", "success").await;

    let req = patch_user(
        &cookies,
        &user_id,
        serde_json::json!({ "displayName": "Bobby" }),
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let after = count_operate_logs_by(&pool, "users.update", "success").await;
    assert_eq!(
        after - before,
        1,
        "user update should write exactly one operate_log row"
    );
}

/// VAL-AUDIT-003: User delete writes one operate_log row.
#[actix_web::test]
async fn user_delete_writes_one_operate_log() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    let _admin_id = seed_user(&pool, "admin", "admin123", "admin", false).await;
    let user_id = seed_user(&pool, "bob", "bobpass", "operator", false).await;
    // Need a second admin to allow deleting one
    let _admin2_id = seed_user(&pool, "admin2", "admin2pass", "admin", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    let before = count_operate_logs_by(&pool, "users.delete", "success").await;

    let req = delete_user(&cookies, &user_id).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let after = count_operate_logs_by(&pool, "users.delete", "success").await;
    assert_eq!(
        after - before,
        1,
        "user delete should write exactly one operate_log row"
    );
}

/// VAL-AUDIT-004: Admin can read /api/operate_logs with filters.
#[actix_web::test]
async fn admin_can_list_operate_logs() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = get_operate_logs(&cookies, "").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert!(
        body.get("items").is_some(),
        "response should have items array"
    );
    assert!(
        body.get("total").is_some(),
        "response should have total count"
    );
    assert!(
        body.get("page").is_some(),
        "response should have page number"
    );
}

/// VAL-AUDIT-004: Admin can filter operate_logs by action.
#[actix_web::test]
async fn admin_can_filter_operate_logs() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = get_operate_logs(&cookies, "?action=auth.login").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    let items = body.get("items").unwrap().as_array().unwrap();
    for item in items {
        assert_eq!(item["action"].as_str().unwrap(), "auth.login");
    }
}

/// VAL-AUDIT-004: Each item in operate_logs response has required fields.
#[actix_web::test]
async fn operate_log_items_have_required_fields() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = get_operate_logs(&cookies, "").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    let items = body.get("items").unwrap().as_array().unwrap();
    assert!(!items.is_empty(), "should have at least one audit log row");

    for item in items {
        assert!(item.get("id").is_some(), "item should have id");
        assert!(
            item.get("ts").is_some() || item.get("createdAt").is_some(),
            "item should have timestamp"
        );
        assert!(item.get("actor").is_some(), "item should have actor");
        assert!(item.get("action").is_some(), "item should have action");
        assert!(item.get("result").is_some(), "item should have result");
    }
}

/// VAL-AUDIT-005: Operator cannot read /api/operate_logs → 403.
#[actix_web::test]
async fn operator_cannot_list_operate_logs() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "operator1", "oppass", "operator", false).await;
    let cookies = do_login!(app, "operator1", "oppass");

    let req = get_operate_logs(&cookies, "").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"].as_str().unwrap(), "FORBIDDEN");
}

/// VAL-AUDIT-005: Viewer cannot read /api/operate_logs → 403.
#[actix_web::test]
async fn viewer_cannot_list_operate_logs() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    seed_user(&pool, "viewer1", "vpass", "viewer", false).await;
    let cookies = do_login!(app, "viewer1", "vpass");

    let req = get_operate_logs(&cookies, "").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"].as_str().unwrap(), "FORBIDDEN");
}

/// VAL-AUDIT-006: No PATCH endpoint on /api/operate_logs (immutable rows).
#[actix_web::test]
async fn operate_log_rows_are_immutable_no_patch() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_cookies(
        test::TestRequest::patch()
            .uri("/api/operate_logs/1")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(serde_json::json!({ "result": "tampered" })),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert!(
        res.status() == StatusCode::NOT_FOUND || res.status() == StatusCode::METHOD_NOT_ALLOWED,
        "PATCH on operate_logs should return 404 or 405, got {}",
        res.status()
    );
}

/// VAL-AUDIT-006: No DELETE endpoint on /api/operate_logs (immutable rows).
#[actix_web::test]
async fn operate_log_rows_are_immutable_no_delete() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = add_cookies(
        test::TestRequest::delete()
            .uri("/api/operate_logs/1")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF)),
        &cookies,
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert!(
        res.status() == StatusCode::NOT_FOUND || res.status() == StatusCode::METHOD_NOT_ALLOWED,
        "DELETE on operate_logs should return 404 or 405, got {}",
        res.status()
    );
}

/// VAL-AUDIT-006: Activation code in log row is redacted.
#[actix_web::test]
async fn activation_code_redacted_in_operate_log_details() {
    let redacted = dt_console_server::operate_log_handlers::redact_activation_code(
        r#"{"code":"ABCD-EFGH-IJKL-MNOP","sku":"enterprise"}"#,
    );

    assert!(
        !redacted.contains("ABCD-EFGH-IJKL-MNOP"),
        "full activation code must not appear in redacted output"
    );
    assert!(
        redacted.contains("<redacted>"),
        "redacted output should contain a redaction marker"
    );
}

/// VAL-SEC-LEAK-001: Password never appears in operate_log details.
#[actix_web::test]
async fn password_not_in_operate_log_details() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let _cookies = do_login!(app, "admin", "admin123");

    // Check all operate_log rows for password leakage
    let logs = OperateLogRepository::list(&pool).await.unwrap();
    for log in &logs {
        if let Some(ref details) = log.details {
            assert!(
                !details.contains("admin123"),
                "operate_log details must not contain password, found in: {details}"
            );
        }
    }

    // Also check via the API endpoint
    let cookies = do_login!(app, "admin", "admin123");
    let req = get_operate_logs(&cookies, "").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(res).await;
    let body_str = body.to_string();
    assert!(
        !body_str.contains("admin123"),
        "API response must not contain password"
    );
}

/// VAL-SEC-LEAK-001: Connection-string passwords redacted in responses.
#[actix_web::test]
async fn connection_string_passwords_redacted() {
    let input = "*************************/db";
    let redacted =
        dt_console_server::operate_log_handlers::redact_connection_string_passwords(input);
    assert!(
        !redacted.contains("secretPw123"),
        "connection string password must be redacted, got: {redacted}"
    );
}

/// VAL-AUDIT-003: Failed mutation also writes an operate_log row (result=failure).
#[actix_web::test]
async fn failed_user_create_writes_failure_audit_log() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    let create_body = serde_json::json!({
        "username": "duplicate_user",
        "password": "pass123",
        "role": "viewer",
        "displayName": "Dup"
    });

    // First create should succeed
    let req = post_user(&cookies, create_body.clone()).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // Second create with same username should fail
    let before = count_operate_logs_by(&pool, "users.create", "failure").await;
    let req = post_user(&cookies, create_body).to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    // Failed create should write a failure audit log
    let after = count_operate_logs_by(&pool, "users.create", "failure").await;
    assert_eq!(
        after - before,
        1,
        "failed user create should write one failure audit log row"
    );
}

/// Operate logs are ordered by created_at DESC (newest first).
#[actix_web::test]
async fn operate_logs_ordered_by_timestamp_desc() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create a user (generates a second log entry after login)
    let req = post_user(
        &cookies,
        serde_json::json!({
            "username": "user1",
            "password": "pass1",
            "role": "viewer",
            "displayName": "User One"
        }),
    )
    .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // Now query the logs
    let req = get_operate_logs(&cookies, "").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    let items = body.get("items").unwrap().as_array().unwrap();
    assert!(items.len() >= 2, "should have at least 2 log entries");

    // Verify newest first (DESC order)
    let first_ts = items[0]
        .get("ts")
        .or_else(|| items[0].get("createdAt"))
        .unwrap()
        .as_str()
        .unwrap();
    let second_ts = items[1]
        .get("ts")
        .or_else(|| items[1].get("createdAt"))
        .unwrap()
        .as_str()
        .unwrap();
    assert!(first_ts >= second_ts, "items should be ordered by ts DESC");
}

/// Anonymous user cannot access /api/operate_logs → 401.
#[actix_web::test]
async fn anonymous_cannot_list_operate_logs() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let req = test::TestRequest::get()
        .uri("/api/operate_logs")
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

/// Admin can filter operate_logs by actor.
#[actix_web::test]
async fn admin_can_filter_operate_logs_by_actor() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = get_operate_logs(&cookies, "?actor=admin").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    let items = body.get("items").unwrap().as_array().unwrap();
    for item in items {
        assert_eq!(item["actor"].as_str().unwrap(), "admin");
    }
}

/// Admin can filter operate_logs by result.
#[actix_web::test]
async fn admin_can_filter_operate_logs_by_result() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    let req = get_operate_logs(&cookies, "?result=success").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    let items = body.get("items").unwrap().as_array().unwrap();
    for item in items {
        assert_eq!(item["result"].as_str().unwrap(), "success");
    }
}

/// Pagination works correctly on operate_logs endpoint.
#[actix_web::test]
async fn operate_logs_pagination_works() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let cookies = do_login!(app, "admin", "admin123");

    // Create several users to generate more log entries
    for i in 0..5 {
        let req = post_user(
            &cookies,
            serde_json::json!({
                "username": format!("user_{i}"),
                "password": format!("pass_{i}"),
                "role": "viewer",
                "displayName": format!("User {i}")
            }),
        )
        .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    // Page 1 with page_size=2
    let req = get_operate_logs(&cookies, "?page=1&page_size=2").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["page"].as_i64().unwrap(), 1);
    assert!(body.get("items").unwrap().as_array().unwrap().len() <= 2);
    assert!(
        body["total"].as_i64().unwrap() > 2,
        "should have more than 2 total entries"
    );
}

/// VAL-AUDIT-002: Failed login operate_log includes failure reason in details.
#[actix_web::test]
async fn failed_login_includes_reason_in_details() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let _cookies = do_login_fail!(app, "admin", "wrongpw", StatusCode::UNAUTHORIZED);

    let logs = OperateLogRepository::list(&pool).await.unwrap();
    let login_log = logs
        .iter()
        .find(|l| l.action == "auth.login" && l.result == "failure")
        .unwrap();

    assert!(
        login_log.details.is_some(),
        "failed login operate_log should have details"
    );
    let details = login_log.details.as_ref().unwrap();
    assert!(
        details.contains("reason"),
        "failed login details should contain reason field"
    );
}

/// Rate-limited login attempt writes operate_log with result=rate_limited.
#[actix_web::test]
async fn rate_limited_login_writes_operate_log() {
    let pool = test_pool().await;

    // Create a separate app with aggressive rate limiting
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

    let limiter = RateLimiter::new(RateLimitConfig {
        max_attempts: 2,
        window_secs: 60,
        block_secs: 60,
    });

    let app = test::init_service(
        App::new()
            .wrap(session_mw)
            .wrap(Csrf)
            .app_data(JsonConfig::default().error_handler(|err, _req| {
                error::ApiError::new(error::codes::PARSE_ERROR, err.to_string()).into()
            }))
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(limiter))
            .app_data(web::Data::new(IDLE_TIMEOUT_SECS))
            .service(
                web::scope("/api")
                    .service(auth_handlers::login)
                    .service(auth_handlers::logout)
                    .service(auth_handlers::me)
                    .service(operate_log_handlers::list_operate_logs),
            ),
    )
    .await;

    seed_user(&pool, "admin", "admin123", "admin", false).await;

    // Fail twice to hit the limit
    let _ = do_login_fail!(app, "admin", "wrong1", StatusCode::UNAUTHORIZED);
    let _ = do_login_fail!(app, "admin", "wrong2", StatusCode::UNAUTHORIZED);

    // Third attempt should be rate-limited
    let before_rate_limited = count_operate_logs_by(&pool, "auth.login", "rate_limited").await;
    let req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "admin".to_string(),
            password: "wrong3".to_string(),
        })
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);

    let after_rate_limited = count_operate_logs_by(&pool, "auth.login", "rate_limited").await;
    assert_eq!(
        after_rate_limited - before_rate_limited,
        1,
        "rate-limited login should write one operate_log with result=rate_limited"
    );
}
