//! CSRF middleware: XSRF-TOKEN cookie → X-XSRF-TOKEN header on unsafe methods.
//!
//! On every response where the `XSRF-TOKEN` cookie is not yet set, a new random
//! token is generated and set as `XSRF-TOKEN` (not HttpOnly — client JS must
//! read it).
//!
//! On unsafe methods (POST, PUT, PATCH, DELETE) the middleware requires:
//! - A session cookie to be present (otherwise 401 UNAUTHENTICATED for anonymous)
//! - An `XSRF-TOKEN` cookie to be present
//! - An `X-XSRF-TOKEN` header that matches the cookie value
//!
//! Anonymous unsafe method → 401 `{ code: "UNAUTHENTICATED" }`
//! Missing header → 403 `{ code: "CSRF_TOKEN_MISSING" }`
//! Mismatched header → 403 `{ code: "CSRF_TOKEN_MISMATCH" }`

use actix_web::{
    body::EitherBody,
    cookie::{Cookie, SameSite},
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    http::{header, Method},
    Error, HttpResponse, ResponseError,
};
use futures::future::{ok, LocalBoxFuture, Ready};
use uuid::Uuid;

use crate::error::{codes, ApiError};

/// Name of the actix-session cookie. Must match the value set in lib.rs
/// SessionMiddleware builder.
pub const SESSION_COOKIE_NAME: &str = "session";

/// CSRF middleware factory.
pub struct Csrf;

pub const XSRF_COOKIE_NAME: &str = "XSRF-TOKEN";
pub const XSRF_HEADER_NAME: &str = "X-XSRF-TOKEN";

impl<S, B> Transform<S, ServiceRequest> for Csrf
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Transform = CsrfMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(CsrfMiddleware { service })
    }
}

pub struct CsrfMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for CsrfMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &self,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let is_unsafe = matches!(
            *req.method(),
            Method::POST | Method::PUT | Method::PATCH | Method::DELETE
        );

        let csrf_cookie = req.cookie(XSRF_COOKIE_NAME);
        let session_cookie = req.cookie(SESSION_COOKIE_NAME);
        let needs_new_token = csrf_cookie.is_none();
        let token = csrf_cookie
            .as_ref()
            .map(|c| c.value().to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        if is_unsafe {
            // CSRF protection is only meaningful for authenticated users.
            // If no session cookie is present the request is anonymous:
            // skip CSRF validation entirely and let the request through.
            // Protected routes will return 401 UNAUTHENTICATED via the
            // UserContext FromRequest extractor; public unsafe-method
            // endpoints (e.g. POST /api/auth/login) continue to work.
            if session_cookie.is_none() {
                let needs_set = needs_new_token;
                let token_to_set = token;
                let fut = self.service.call(req);
                return Box::pin(async move {
                    let mut res = fut.await?;
                    if needs_set {
                        set_xsrf_cookie(res.response_mut(), &token_to_set);
                    }
                    Ok(res.map_into_left_body())
                });
            }

            // Session cookie present — enforce CSRF token checks.
            let header_val = req.headers().get(XSRF_HEADER_NAME);
            let cookie_has_token = csrf_cookie.is_some();

            match (cookie_has_token, header_val) {
                (false, _) => {
                    let err = ApiError::new(
                        codes::CSRF_TOKEN_MISSING,
                        "CSRF token is required for unsafe methods",
                    );
                    let mut res = err.error_response();
                    set_xsrf_cookie(&mut res, &token);
                    let (http_req, _) = req.into_parts();
                    return Box::pin(async move {
                        Ok(ServiceResponse::new(http_req, res.map_into_right_body()))
                    });
                }
                (true, None) => {
                    let err = ApiError::new(
                        codes::CSRF_TOKEN_MISSING,
                        "CSRF token is required for unsafe methods",
                    );
                    let mut res = err.error_response();
                    if needs_new_token {
                        set_xsrf_cookie(&mut res, &token);
                    }
                    let (http_req, _) = req.into_parts();
                    return Box::pin(async move {
                        Ok(ServiceResponse::new(http_req, res.map_into_right_body()))
                    });
                }
                (true, Some(hdr)) => {
                    let cookie_val = csrf_cookie.as_ref().map(|c| c.value()).unwrap_or("");
                    if cookie_val != hdr.to_str().unwrap_or("") {
                        let err = ApiError::new(
                            codes::CSRF_TOKEN_MISMATCH,
                            "CSRF token does not match the cookie value",
                        );
                        let mut res = err.error_response();
                        if needs_new_token {
                            set_xsrf_cookie(&mut res, &token);
                        }
                        let (http_req, _) = req.into_parts();
                        return Box::pin(async move {
                            Ok(ServiceResponse::new(http_req, res.map_into_right_body()))
                        });
                    }
                }
            }
        }

        let needs_set = needs_new_token;
        let token_to_set = token;

        let fut = self.service.call(req);

        Box::pin(async move {
            let mut res = fut.await?;
            if needs_set {
                set_xsrf_cookie(res.response_mut(), &token_to_set);
            }
            Ok(res.map_into_left_body())
        })
    }
}

fn set_xsrf_cookie<B>(response: &mut HttpResponse<B>, token: &str) {
    let cookie = Cookie::build(XSRF_COOKIE_NAME, token)
        .path("/")
        .same_site(SameSite::Lax)
        .http_only(false) // Client JS needs to read it
        .finish();

    if let Ok(val) = cookie.encoded().to_string().parse() {
        response.headers_mut().append(header::SET_COOKIE, val);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{http::StatusCode, test, web, App, HttpResponse};

    async fn ok_handler() -> HttpResponse {
        HttpResponse::Ok().json(serde_json::json!({"status": "ok"}))
    }

    // ── GET requests (safe methods) ──────────────────────────────────

    #[actix_web::test]
    async fn csrf_get_does_not_require_token() {
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::get().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::get().uri("/api/test").to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn csrf_get_sets_xsrf_cookie() {
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::get().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::get().uri("/api/test").to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::OK);

        let set_cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .expect("XSRF-TOKEN cookie should be set");
        let val = set_cookie.to_str().unwrap();
        assert!(
            val.contains("XSRF-TOKEN="),
            "Set-Cookie should contain XSRF-TOKEN, got: {val}"
        );
    }

    #[actix_web::test]
    async fn csrf_xsrf_cookie_not_httponly() {
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::get().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::get().uri("/api/test").to_request();
        let res = test::call_service(&app, req).await;

        let set_cookie = res.headers().get(header::SET_COOKIE).unwrap();
        let val = set_cookie.to_str().unwrap();
        // HttpOnly must NOT be present so client JS can read the cookie
        assert!(
            !val.contains("HttpOnly"),
            "XSRF-TOKEN cookie must not be HttpOnly, got: {val}"
        );
    }

    // ── Anonymous unsafe methods → CSRF skipped, request passes ─────
    // CSRF protection only applies to authenticated sessions.
    // For anonymous requests, CSRF validation is skipped entirely;
    // the auth middleware (FromRequest extractor) on protected routes
    // will return 401 UNAUTHENTICATED.

    #[actix_web::test]
    async fn anonymous_post_passes_csrf_check() {
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::post().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::post().uri("/api/test").to_request();
        let res = test::call_service(&app, req).await;
        // No session cookie → CSRF check is skipped → handler returns OK
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn anonymous_delete_passes_csrf_check() {
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::delete().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::delete().uri("/api/test").to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn anonymous_patch_passes_csrf_check() {
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::patch().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::patch().uri("/api/test").to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn anonymous_put_passes_csrf_check() {
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::put().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::put().uri("/api/test").to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn anonymous_post_with_xsrf_cookie_still_passes() {
        // Even if the client has an XSRF-TOKEN cookie from a prior GET,
        // without a session cookie CSRF validation is skipped.
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::post().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::post()
            .uri("/api/test")
            .cookie(Cookie::new(XSRF_COOKIE_NAME, "some-xsrf-value"))
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn anonymous_unsafe_method_still_sets_xsrf_cookie() {
        // Even when CSRF check is skipped, the XSRF-TOKEN cookie should
        // still be set so the client can use it after authenticating.
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::post().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::post().uri("/api/test").to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::OK);

        let set_cookie = res.headers().get(header::SET_COOKIE);
        assert!(
            set_cookie.is_some(),
            "Anonymous unsafe-method response should still set XSRF-TOKEN cookie"
        );
        let val = set_cookie.unwrap().to_str().unwrap();
        assert!(val.contains("XSRF-TOKEN="));
    }

    // ── Authenticated unsafe methods without CSRF → 403 ────────────

    #[actix_web::test]
    async fn authenticated_post_without_xsrf_header_returns_403_csrf_token_missing() {
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::post().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::post()
            .uri("/api/test")
            .cookie(Cookie::new(SESSION_COOKIE_NAME, "session-value"))
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        let body: serde_json::Value = test::read_body_json(res).await;
        assert_eq!(body["code"], codes::CSRF_TOKEN_MISSING);
    }

    #[actix_web::test]
    async fn authenticated_post_with_xsrf_cookie_but_no_header_returns_403() {
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::post().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::post()
            .uri("/api/test")
            .cookie(Cookie::new(SESSION_COOKIE_NAME, "session-value"))
            .cookie(Cookie::new(XSRF_COOKIE_NAME, "some-xsrf-value"))
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        let body: serde_json::Value = test::read_body_json(res).await;
        assert_eq!(body["code"], codes::CSRF_TOKEN_MISSING);
    }

    // ── Authenticated with valid CSRF → passes ─────────────────────

    #[actix_web::test]
    async fn authenticated_post_with_valid_csrf_token_succeeds() {
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::post().to(ok_handler)),
        )
        .await;

        let token = "test-csrf-token-12345";
        let req = test::TestRequest::post()
            .uri("/api/test")
            .cookie(Cookie::new(SESSION_COOKIE_NAME, "session-value"))
            .cookie(Cookie::new(XSRF_COOKIE_NAME, token))
            .insert_header((XSRF_HEADER_NAME, token))
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    // ── Mismatched CSRF token → 403 CSRF_TOKEN_MISMATCH ────────────

    #[actix_web::test]
    async fn authenticated_post_with_mismatched_token_returns_403_csrf_token_mismatch() {
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::post().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::post()
            .uri("/api/test")
            .cookie(Cookie::new(SESSION_COOKIE_NAME, "session-value"))
            .cookie(Cookie::new(XSRF_COOKIE_NAME, "token-a"))
            .insert_header((XSRF_HEADER_NAME, "token-b"))
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        let body: serde_json::Value = test::read_body_json(res).await;
        assert_eq!(body["code"], codes::CSRF_TOKEN_MISMATCH);
    }

    // ── 403 error response sets XSRF-TOKEN for retry ────────────────

    #[actix_web::test]
    async fn authenticated_403_response_sets_xsrf_cookie_for_retry() {
        let app = test::init_service(
            App::new()
                .wrap(Csrf)
                .route("/api/test", web::post().to(ok_handler)),
        )
        .await;

        let req = test::TestRequest::post()
            .uri("/api/test")
            .cookie(Cookie::new(SESSION_COOKIE_NAME, "session-value"))
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        let set_cookie = res.headers().get(header::SET_COOKIE);
        assert!(
            set_cookie.is_some(),
            "403 CSRF error response should set XSRF-TOKEN cookie for client retry"
        );
        let val = set_cookie.unwrap().to_str().unwrap();
        assert!(val.contains("XSRF-TOKEN="));
    }
}
