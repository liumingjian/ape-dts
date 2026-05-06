//! Integration tests for the authentication flow: login, logout, /me,
//! session management, rate limiting, bcrypt verification, and role middleware.
//!
//! These tests exercise the full HTTP stack (handlers + middleware + DB)
//! against an in-memory SQLite database.

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
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use sqlx::SqlitePool;

// ─── Helpers ─────────────────────────────────────────────────────────────

/// Create a migrated test pool backed by a temp file.
async fn test_pool() -> SqlitePool {
    let dir = std::env::temp_dir().join("dt-console-server-auth-test");
    std::fs::create_dir_all(&dir).unwrap();
    let test_name = std::thread::current()
        .name()
        .unwrap_or("unknown")
        .to_string();
    let safe_name: String = test_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let path = dir.join(format!("auth-{safe_name}.db"));
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
                .service(auth_handlers::me),
        )
}

/// Build a test app with a custom rate limiter.
fn build_test_app_with_rate_limit(
    pool: SqlitePool,
    limiter: RateLimiter,
    idle_timeout: i64,
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
        .app_data(web::Data::new(limiter))
        .app_data(web::Data::new(idle_timeout))
        .service(
            web::scope("/api")
                .service(auth_handlers::login)
                .service(auth_handlers::logout)
                .service(auth_handlers::me),
        )
}

/// Seed a user and return user_id.
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

// ─── VAL-AUTH-001: Successful login with valid credentials ──────────────

#[actix_web::test]
async fn login_valid_credentials_returns_200_with_user_info() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "admin".to_string(),
            password: "admin123".to_string(),
        })
        .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["username"], "admin");
    assert_eq!(body["role"], "admin");
    assert!(body.get("password").is_none(), "password must not appear");
    assert!(body.get("passwordHash").is_none(), "hash must not appear");
}

// ─── VAL-AUTH-002: Login with wrong password ─────────────────────────────

#[actix_web::test]
async fn login_wrong_password_returns_401() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "admin".to_string(),
            password: "wrongpassword".to_string(),
        })
        .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::INVALID_CREDENTIALS);
}

// ─── VAL-AUTH-003: Login with unknown username ───────────────────────────

#[actix_web::test]
async fn login_unknown_username_returns_401_same_code() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "nonexistent".to_string(),
            password: "anything".to_string(),
        })
        .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::INVALID_CREDENTIALS);
}

// ─── VAL-AUTH-004: Disabled account cannot login ────────────────────────

#[actix_web::test]
async fn login_disabled_user_returns_401_account_disabled() {
    let pool = test_pool().await;
    seed_user(&pool, "disabled_user", "pass123", "viewer", true).await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "disabled_user".to_string(),
            password: "pass123".to_string(),
        })
        .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::ACCOUNT_DISABLED);
}

// ─── VAL-AUTH-005: Session cookie carries security flags ────────────────

#[actix_web::test]
async fn session_cookie_has_httponly_samesite_lax_path() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "admin".to_string(),
            password: "admin123".to_string(),
        })
        .to_request();

    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Find the session= cookie in Set-Cookie headers
    let session_cookie_str = res
        .headers()
        .get_all("set-cookie")
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.contains("session="))
        .expect("should have a session= cookie");

    assert!(
        session_cookie_str.contains("HttpOnly"),
        "expected HttpOnly in: {session_cookie_str}"
    );
    assert!(
        session_cookie_str.contains("SameSite=Lax"),
        "expected SameSite=Lax in: {session_cookie_str}"
    );
    assert!(
        session_cookie_str.contains("Path=/"),
        "expected Path=/ in: {session_cookie_str}"
    );

    // Cookie must NOT embed username, password, or bcrypt hash
    assert!(
        !session_cookie_str.contains("admin123"),
        "cookie must not contain password"
    );
    assert!(
        !session_cookie_str.contains("$2b$"),
        "cookie must not contain bcrypt hash"
    );
}

// ─── VAL-AUTH-006: Session persists across requests ──────────────────────

#[actix_web::test]
async fn session_persists_across_requests() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    // Login
    let login_req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "admin".to_string(),
            password: "admin123".to_string(),
        })
        .to_request();
    let login_res = test::call_service(&app, login_req).await;
    assert_eq!(login_res.status(), StatusCode::OK);
    let cookies = collect_cookies(&login_res);

    // GET /api/auth/me with the cookies
    let me_req = add_cookies(test::TestRequest::get().uri("/api/auth/me"), &cookies).to_request();
    let me_res = test::call_service(&app, me_req).await;
    assert_eq!(me_res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(me_res).await;
    assert_eq!(body["username"], "admin");
    assert_eq!(body["role"], "admin");
}

// ─── VAL-AUTH-007: Logout invalidates session ────────────────────────────

#[actix_web::test]
async fn logout_invalidates_session() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    // Login
    let login_req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "admin".to_string(),
            password: "admin123".to_string(),
        })
        .to_request();
    let login_res = test::call_service(&app, login_req).await;
    let cookies = collect_cookies(&login_res);

    // Logout
    let logout_req = add_cookies(
        test::TestRequest::post()
            .uri("/api/auth/logout")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF)),
        &cookies,
    )
    .to_request();
    let logout_res = test::call_service(&app, logout_req).await;
    assert_eq!(logout_res.status(), StatusCode::OK);

    // Try to reuse the old session → 401
    let me_req = add_cookies(test::TestRequest::get().uri("/api/auth/me"), &cookies).to_request();
    let me_res = test::call_service(&app, me_req).await;
    assert_eq!(me_res.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = test::read_body_json(me_res).await;
    assert_eq!(body["code"], error::codes::UNAUTHENTICATED);
}

// ─── VAL-AUTH-008: Anonymous /api/auth/me returns 401 ────────────────────

#[actix_web::test]
async fn me_anonymous_returns_401() {
    let pool = test_pool().await;
    let app = test::init_service(build_test_app(pool)).await;

    let req = test::TestRequest::get().uri("/api/auth/me").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::UNAUTHENTICATED);
}

// ─── VAL-AUTH-009: Idle session expires ──────────────────────────────────

#[tokio::test]
async fn idle_session_expires() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let short_timeout: i64 = 1;
    let app = test::init_service(build_test_app_with_rate_limit(
        pool,
        RateLimiter::new(RateLimitConfig::default()),
        short_timeout,
    ))
    .await;

    // Login
    let login_req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "admin".to_string(),
            password: "admin123".to_string(),
        })
        .to_request();
    let login_res = test::call_service(&app, login_req).await;
    assert_eq!(login_res.status(), StatusCode::OK);
    let cookies = collect_cookies(&login_res);

    // Wait for idle timeout
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Try /api/auth/me → should be expired
    let me_req = add_cookies(test::TestRequest::get().uri("/api/auth/me"), &cookies).to_request();
    let me_res = test::call_service(&app, me_req).await;
    assert_eq!(me_res.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = test::read_body_json(me_res).await;
    assert_eq!(body["code"], error::codes::SESSION_EXPIRED);
}

// ─── VAL-AUTH-010: Replay after logout rejected ──────────────────────────

#[actix_web::test]
async fn login_replay_after_logout_rejected() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    // Login
    let login_req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "admin".to_string(),
            password: "admin123".to_string(),
        })
        .to_request();
    let login_res = test::call_service(&app, login_req).await;
    let cookies = collect_cookies(&login_res);

    // Logout
    let logout_req = add_cookies(
        test::TestRequest::post()
            .uri("/api/auth/logout")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF)),
        &cookies,
    )
    .to_request();
    let _ = test::call_service(&app, logout_req).await;

    // Reuse old cookies → 401
    let me_req = add_cookies(test::TestRequest::get().uri("/api/auth/me"), &cookies).to_request();
    let me_res = test::call_service(&app, me_req).await;
    assert_eq!(me_res.status(), StatusCode::UNAUTHORIZED);
}

// ─── VAL-SEC-AUTH-002: Login rotates session ID ──────────────────────────

#[actix_web::test]
async fn login_rotates_session_id() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;
    let app = test::init_service(build_test_app(pool)).await;

    // Login first time
    let login1_req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "admin".to_string(),
            password: "admin123".to_string(),
        })
        .to_request();
    let login1_res = test::call_service(&app, login1_req).await;
    assert_eq!(login1_res.status(), StatusCode::OK);
    let cookies1 = collect_cookies(&login1_res);

    // Verify session works
    let me1_req = add_cookies(test::TestRequest::get().uri("/api/auth/me"), &cookies1).to_request();
    let me1_res = test::call_service(&app, me1_req).await;
    assert_eq!(me1_res.status(), StatusCode::OK);

    // Login again (triggers session rotation)
    let login2_req = add_cookies(
        test::TestRequest::post()
            .uri("/api/auth/login")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(LoginRequest {
                username: "admin".to_string(),
                password: "admin123".to_string(),
            }),
        &cookies1,
    )
    .to_request();
    let login2_res = test::call_service(&app, login2_req).await;
    assert_eq!(login2_res.status(), StatusCode::OK);
    let cookies2 = collect_cookies(&login2_res);

    // New session should work
    let me2_req = add_cookies(test::TestRequest::get().uri("/api/auth/me"), &cookies2).to_request();
    let me2_res = test::call_service(&app, me2_req).await;
    assert_eq!(me2_res.status(), StatusCode::OK);
}

// ─── VAL-SEC-AUTH-003: Rate limit per username+IP ────────────────────────

#[actix_web::test]
async fn rate_limit_blocks_after_max_failed_attempts() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let strict_limiter = RateLimiter::new(RateLimitConfig {
        max_attempts: 3,
        window_secs: 60,
        block_secs: 60,
    });
    let app = test::init_service(build_test_app_with_rate_limit(
        pool,
        strict_limiter,
        IDLE_TIMEOUT_SECS,
    ))
    .await;

    // Make 3 failed login attempts
    for _ in 0..3 {
        let req = test::TestRequest::post()
            .uri("/api/auth/login")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
            .insert_header((XSRF_HEADER_NAME, XSRF))
            .set_json(LoginRequest {
                username: "admin".to_string(),
                password: "wrong".to_string(),
            })
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    // 4th attempt → 429
    let req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "admin".to_string(),
            password: "wrong".to_string(),
        })
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], error::codes::TOO_MANY_ATTEMPTS);
    assert!(body["details"]["retry_after_secs"].is_number());

    // Even with correct password → still blocked
    let correct_req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "admin".to_string(),
            password: "admin123".to_string(),
        })
        .to_request();
    let correct_res = test::call_service(&app, correct_req).await;
    assert_eq!(correct_res.status(), StatusCode::TOO_MANY_REQUESTS);
}

// ─── Bcrypt cost ≥ 10 verified ────────────────────────────────────────────

#[tokio::test]
async fn stored_password_hash_uses_bcrypt_cost_at_least_10() {
    let pool = test_pool().await;
    let user_id = seed_user(&pool, "testuser", "mypassword", "viewer", false).await;

    let user = dt_console_server::repositories::user_repository::UserRepository::find_by_id(
        &pool, &user_id,
    )
    .await
    .unwrap();

    assert!(
        user.password_hash.starts_with("$2b$") || user.password_hash.starts_with("$2a$"),
        "password hash should be bcrypt, got: {}",
        user.password_hash
    );
    let parts: Vec<&str> = user.password_hash.split('$').collect();
    let cost: u32 = parts[2].parse().unwrap();
    assert!(cost >= 10, "bcrypt cost should be ≥ 10, got {cost}");

    // Verify the password actually works
    assert!(bcrypt::verify("mypassword", &user.password_hash).unwrap());
}

// ─── Disabled account cannot use existing session ────────────────────────

#[actix_web::test]
async fn disabled_user_existing_session_rejected() {
    let pool = test_pool().await;
    seed_user(&pool, "operator", "pass123", "operator", false).await;
    let app = test::init_service(build_test_app(pool.clone())).await;

    // Login
    let login_req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "operator".to_string(),
            password: "pass123".to_string(),
        })
        .to_request();
    let login_res = test::call_service(&app, login_req).await;
    assert_eq!(login_res.status(), StatusCode::OK);
    let cookies = collect_cookies(&login_res);

    // Verify session works
    let me1_req = add_cookies(test::TestRequest::get().uri("/api/auth/me"), &cookies).to_request();
    let me1_res = test::call_service(&app, me1_req).await;
    assert_eq!(me1_res.status(), StatusCode::OK);

    // Disable the user in DB
    let user = dt_console_server::repositories::user_repository::UserRepository::find_by_username(
        &pool, "operator",
    )
    .await
    .unwrap();
    auth::disable_user(&pool, &user.id).await.unwrap();

    // Existing session should be rejected (sessions were invalidated)
    let me2_req = add_cookies(test::TestRequest::get().uri("/api/auth/me"), &cookies).to_request();
    let me2_res = test::call_service(&app, me2_req).await;
    assert_eq!(me2_res.status(), StatusCode::UNAUTHORIZED);
}

// ─── Password reset takes effect immediately ──────────────────────────────

#[tokio::test]
async fn password_reset_takes_effect_immediately() {
    let pool = test_pool().await;
    let user_id = seed_user(&pool, "operator", "oldpassword", "operator", false).await;

    // Reset password
    auth::reset_password(&pool, &user_id, "newpassword")
        .await
        .unwrap();

    // Login with new password
    let app = test::init_service(build_test_app(pool)).await;
    let req = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "operator".to_string(),
            password: "newpassword".to_string(),
        })
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Login with old password should fail
    let req2 = test::TestRequest::post()
        .uri("/api/auth/login")
        .cookie(Cookie::new(XSRF_COOKIE_NAME, XSRF))
        .insert_header((XSRF_HEADER_NAME, XSRF))
        .set_json(LoginRequest {
            username: "operator".to_string(),
            password: "oldpassword".to_string(),
        })
        .to_request();
    let res2 = test::call_service(&app, req2).await;
    assert_eq!(res2.status(), StatusCode::UNAUTHORIZED);
}

// ─── Cannot delete last admin ─────────────────────────────────────────────

#[tokio::test]
async fn cannot_delete_last_admin() {
    let pool = test_pool().await;
    let admin_id = seed_user(&pool, "onlyadmin", "admin123", "admin", false).await;

    let result = auth::delete_user(&pool, &admin_id).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, error::codes::LAST_ADMIN_PROTECTED);
}

#[tokio::test]
async fn can_delete_admin_when_another_exists() {
    let pool = test_pool().await;
    let admin1_id = seed_user(&pool, "admin1", "pass1", "admin", false).await;
    let admin2_id = seed_user(&pool, "admin2", "pass2", "admin", false).await;

    auth::delete_user(&pool, &admin1_id).await.unwrap();

    // Last one still can't be deleted
    let result = auth::delete_user(&pool, &admin2_id).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, error::codes::LAST_ADMIN_PROTECTED);
}

// ─── Audit log written on login ──────────────────────────────────────────

#[tokio::test]
async fn successful_login_writes_operate_log() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let rate_limiter = RateLimiter::new(RateLimitConfig::default());
    let result = auth::login(&pool, &rate_limiter, "admin", "admin123", "127.0.0.1", 3600).await;
    assert!(result.is_ok());

    let logs =
        dt_console_server::repositories::operate_log_repository::OperateLogRepository::list(&pool)
            .await
            .unwrap();
    let login_log = logs
        .iter()
        .find(|l| l.action == "auth.login" && l.result == "success");
    assert!(login_log.is_some());
    let log = login_log.unwrap();
    assert_eq!(log.actor, "admin");
    assert_eq!(log.ip.as_deref(), Some("127.0.0.1"));
    // Password must never appear in details
    if let Some(ref details) = log.details {
        assert!(!details.contains("admin123"));
    }
}

#[tokio::test]
async fn failed_login_writes_operate_log() {
    let pool = test_pool().await;
    seed_user(&pool, "admin", "admin123", "admin", false).await;

    let rate_limiter = RateLimiter::new(RateLimitConfig::default());
    let _ = auth::login(
        &pool,
        &rate_limiter,
        "admin",
        "wrongpass",
        "127.0.0.1",
        3600,
    )
    .await;

    let logs =
        dt_console_server::repositories::operate_log_repository::OperateLogRepository::list(&pool)
            .await
            .unwrap();
    let fail_log = logs
        .iter()
        .find(|l| l.action == "auth.login" && l.result == "failure");
    assert!(fail_log.is_some());
}

// ─── Seed admin works ─────────────────────────────────────────────────────

#[tokio::test]
async fn seed_admin_creates_default_admin() {
    let pool = test_pool().await;
    auth::seed_admin(&pool).await.unwrap();

    let user = dt_console_server::repositories::user_repository::UserRepository::find_by_username(
        &pool, "admin",
    )
    .await
    .unwrap();
    assert_eq!(user.role, "admin");
    assert!(!user.disabled);
    assert!(bcrypt::verify("admin123", &user.password_hash).unwrap());
}

#[tokio::test]
async fn seed_admin_idempotent() {
    let pool = test_pool().await;
    auth::seed_admin(&pool).await.unwrap();
    auth::seed_admin(&pool).await.unwrap();

    let users = dt_console_server::repositories::user_repository::UserRepository::list(&pool)
        .await
        .unwrap();
    assert_eq!(users.len(), 1);
}
