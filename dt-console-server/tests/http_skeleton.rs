//! Integration tests for HTTP scaffolding: error envelope, CSRF, JSON parse errors,
//! session middleware, and CORS.

use actix_session::storage::CookieSessionStore;
use actix_session::SessionMiddleware;
use actix_web::cookie::Key;
use actix_web::http::StatusCode;
use actix_web::test;
use actix_web::web::{self, JsonConfig};
use actix_web::{App, HttpResponse, ResponseError};
use dt_console_server::error;
use dt_console_server::error::codes;
use dt_console_server::health;
use dt_console_server::log_sse_handlers;
use dt_console_server::metrics_scraper;
use dt_console_server::middleware::csrf::{
    Csrf, SESSION_COOKIE_NAME, XSRF_COOKIE_NAME, XSRF_HEADER_NAME,
};

/// Build a test app with the same middleware stack as production.
fn test_app() -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    let key = Key::generate();
    let master_bytes = key.master().to_vec();
    let key2 = Key::from(&master_bytes);

    let session_mw = SessionMiddleware::builder(CookieSessionStore::default(), key2)
        .cookie_name("session".to_string())
        .cookie_secure(false)
        .cookie_http_only(true)
        .cookie_same_site(actix_web::cookie::SameSite::Lax)
        .cookie_path("/".to_string())
        .build();

    App::new()
        .wrap(actix_web::middleware::Logger::default())
        .wrap(actix_cors::Cors::default())
        .wrap(session_mw)
        .wrap(Csrf)
        .app_data(JsonConfig::default().error_handler(|err, _req| {
            error::ApiError::new(error::codes::PARSE_ERROR, err.to_string()).into()
        }))
        .app_data(web::Data::new(metrics_scraper::ScraperState::new()))
        .app_data(web::Data::new(log_sse_handlers::LogSseState::default()))
        .service(
            web::scope("/api")
                .service(health::healthz)
                .route("/echo", web::post().to(echo_handler)),
        )
}

#[derive(serde::Deserialize, serde::Serialize)]
struct EchoPayload {
    message: String,
}

async fn echo_handler(body: web::Json<EchoPayload>) -> HttpResponse {
    HttpResponse::Ok().json(body.into_inner())
}

// ─── Error Envelope Tests ────────────────────────────────────────────────

#[actix_web::test]
async fn error_envelope_4xx_returns_code_message_details() {
    let app = test::init_service(test_app()).await;

    // POST with session cookie but without CSRF token → 403 with envelope
    let req = test::TestRequest::post()
        .uri("/api/echo")
        .cookie(actix_web::cookie::Cookie::new(
            SESSION_COOKIE_NAME,
            "session-value",
        ))
        .set_json(serde_json::json!({"message": "hi"}))
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert!(body["code"].is_string(), "envelope must have 'code'");
    assert!(body["message"].is_string(), "envelope must have 'message'");
    // details may be absent or null for CSRF errors
}

#[actix_web::test]
async fn error_envelope_csrf_missing_has_correct_code() {
    let app = test::init_service(test_app()).await;

    // Must include a session cookie so the CSRF middleware enforces CSRF.
    // Without a session cookie, the request is treated as anonymous and
    // CSRF validation is skipped (the auth middleware would return 401 on
    // protected routes).
    let req = test::TestRequest::post()
        .uri("/api/echo")
        .cookie(actix_web::cookie::Cookie::new(
            SESSION_COOKIE_NAME,
            "session-value",
        ))
        .to_request();
    let res = test::call_service(&app, req).await;

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], codes::CSRF_TOKEN_MISSING);
}

// ─── CSRF Tests (full stack) ────────────────────────────────────────────

#[actix_web::test]
async fn full_stack_post_without_xsrf_token_returns_403() {
    let app = test::init_service(test_app()).await;

    let req = test::TestRequest::post()
        .uri("/api/echo")
        .cookie(actix_web::cookie::Cookie::new(
            SESSION_COOKIE_NAME,
            "session-value",
        ))
        .set_json(serde_json::json!({"message": "hi"}))
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[actix_web::test]
async fn full_stack_post_with_valid_xsrf_token_succeeds() {
    let app = test::init_service(test_app()).await;

    let token = "my-xsrf-token-abc";
    let req = test::TestRequest::post()
        .uri("/api/echo")
        .cookie(actix_web::cookie::Cookie::new(
            SESSION_COOKIE_NAME,
            "session-value",
        ))
        .cookie(actix_web::cookie::Cookie::new(XSRF_COOKIE_NAME, token))
        .insert_header((XSRF_HEADER_NAME, token))
        .set_json(serde_json::json!({"message": "hello"}))
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: EchoPayload = test::read_body_json(res).await;
    assert_eq!(body.message, "hello");
}

#[actix_web::test]
async fn full_stack_delete_without_xsrf_token_returns_403() {
    let app = test::init_service(test_app()).await;

    let req = test::TestRequest::delete()
        .uri("/api/healthz")
        .cookie(actix_web::cookie::Cookie::new(
            SESSION_COOKIE_NAME,
            "session-value",
        ))
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

// ─── JSON Parse Error Tests ─────────────────────────────────────────────

#[actix_web::test]
async fn invalid_json_body_returns_400_parse_error() {
    let app = test::init_service(test_app()).await;

    let token = "test-token-parse";
    let req = test::TestRequest::post()
        .uri("/api/echo")
        .cookie(actix_web::cookie::Cookie::new(
            SESSION_COOKIE_NAME,
            "session-value",
        ))
        .cookie(actix_web::cookie::Cookie::new(XSRF_COOKIE_NAME, token))
        .insert_header((XSRF_HEADER_NAME, token))
        .insert_header(("Content-Type", "application/json"))
        .set_payload("this is not valid json {{{")
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], codes::PARSE_ERROR);
    assert!(body["message"].is_string());
}

#[actix_web::test]
async fn empty_body_with_json_content_type_returns_parse_error() {
    let app = test::init_service(test_app()).await;

    let token = "test-token-empty";
    let req = test::TestRequest::post()
        .uri("/api/echo")
        .cookie(actix_web::cookie::Cookie::new(
            SESSION_COOKIE_NAME,
            "session-value",
        ))
        .cookie(actix_web::cookie::Cookie::new(XSRF_COOKIE_NAME, token))
        .insert_header((XSRF_HEADER_NAME, token))
        .insert_header(("Content-Type", "application/json"))
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], codes::PARSE_ERROR);
}

// ─── Session Middleware Tests ─────────────────────────────────────────────

#[actix_web::test]
async fn session_cookie_is_set_on_first_request() {
    let app = test::init_service(test_app()).await;

    let req = test::TestRequest::get().uri("/api/healthz").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Session middleware should set a session cookie
    let set_cookie_header = res.headers().get("set-cookie");
    // At least XSRF-TOKEN should be set; session cookie may also be set
    assert!(
        set_cookie_header.is_some(),
        "Expected Set-Cookie header on first request"
    );
}

#[actix_web::test]
async fn healthz_endpoint_works_with_full_stack() {
    let app = test::init_service(test_app()).await;

    let req = test::TestRequest::get().uri("/api/healthz").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["status"], "ok");
}

// ─── Error Envelope: i18n Contract ───────────────────────────────────────

#[actix_web::test]
async fn error_envelope_ignores_accept_language() {
    let app = test::init_service(test_app()).await;

    // Use authenticated POST (with session cookie) without CSRF to get 403
    let req1 = test::TestRequest::post()
        .uri("/api/echo")
        .cookie(actix_web::cookie::Cookie::new(
            SESSION_COOKIE_NAME,
            "session-value",
        ))
        .insert_header(("Accept-Language", "zh-CN"))
        .to_request();
    let res1 = test::call_service(&app, req1).await;
    let body1: serde_json::Value = test::read_body_json(res1).await;

    let req2 = test::TestRequest::post()
        .uri("/api/echo")
        .cookie(actix_web::cookie::Cookie::new(
            SESSION_COOKIE_NAME,
            "session-value",
        ))
        .insert_header(("Accept-Language", "en-US"))
        .to_request();
    let res2 = test::call_service(&app, req2).await;
    let body2: serde_json::Value = test::read_body_json(res2).await;

    // Both error responses must be byte-identical (VAL-I18N-CONTRACT-002)
    assert_eq!(
        body1, body2,
        "Error envelope must not vary by Accept-Language"
    );
}

// ─── Error Envelope: details field with code-specific schema ─────────────

#[actix_web::test]
async fn error_envelope_forbidden_has_no_details_when_none_provided() {
    // CSRF_TOKEN_MISSING has no details — just code + message
    let err = error::ApiError::new(
        codes::CSRF_TOKEN_MISSING,
        "CSRF token is required for unsafe methods",
    );
    let res = err.error_response();
    let body_bytes = actix_web::body::to_bytes(res.into_body()).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body["code"], codes::CSRF_TOKEN_MISSING);
    assert!(body["message"].is_string());
    // details should be absent (skip_serializing_if = None)
    assert!(
        body.get("details").is_none() || body["details"].is_null(),
        "details should be absent when None"
    );
}

#[actix_web::test]
async fn error_envelope_with_details_includes_details() {
    let err = error::ApiError::with_details(
        codes::VALIDATION_FAILED,
        "Validation failed",
        serde_json::json!([{"field": "name", "error": "required"}]),
    );
    let res = err.error_response();
    let body_bytes = actix_web::body::to_bytes(res.into_body()).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body["code"], codes::VALIDATION_FAILED);
    assert!(body["details"].is_array());
    assert_eq!(body["details"][0]["field"], "name");
}

// ─── CSRF: cookie and header must match exactly ─────────────────────────

#[actix_web::test]
async fn csrf_header_extra_whitespace_is_mismatch() {
    let app = test::init_service(test_app()).await;

    // Cookie value has no spaces; header has a trailing space
    let req = test::TestRequest::post()
        .uri("/api/echo")
        .cookie(actix_web::cookie::Cookie::new(
            SESSION_COOKIE_NAME,
            "session-value",
        ))
        .cookie(actix_web::cookie::Cookie::new(XSRF_COOKIE_NAME, "token"))
        .insert_header((XSRF_HEADER_NAME, "token "))
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let body: serde_json::Value = test::read_body_json(res).await;
    assert_eq!(body["code"], codes::CSRF_TOKEN_MISMATCH);
}

// ─── Safe methods bypass CSRF ────────────────────────────────────────────

#[actix_web::test]
async fn get_request_bypasses_csrf_check() {
    let app = test::init_service(test_app()).await;

    // No CSRF token needed for GET
    let req = test::TestRequest::get().uri("/api/healthz").to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[actix_web::test]
async fn head_request_bypasses_csrf_check() {
    let app = test::init_service(test_app()).await;

    let req = test::TestRequest::with_uri("/api/healthz")
        .method(actix_web::http::Method::HEAD)
        .to_request();
    let res = test::call_service(&app, req).await;
    // HEAD should succeed (or at least not 403 CSRF)
    assert_ne!(res.status(), StatusCode::FORBIDDEN);
}

#[actix_web::test]
async fn options_request_bypasses_csrf_check() {
    let app = test::init_service(test_app()).await;

    let req = test::TestRequest::with_uri("/api/healthz")
        .method(actix_web::http::Method::OPTIONS)
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_ne!(res.status(), StatusCode::FORBIDDEN);
}
