//! Auth middleware: validates server-side sessions and extracts UserContext.
//!
//! This extractor reads the server-side session token from the actix-session,
//! validates it against the database, and returns a `UserContext`.
//!
//! Handlers that require authentication extract `UserContext` via the
//! `FromRequest` implementation. Anonymous requests to protected endpoints
//! receive 401 UNAUTHENTICATED with the standard error envelope.

use actix_session::Session;
use actix_web::dev::Payload;
use actix_web::http::StatusCode;
use actix_web::{Error, FromRequest, HttpRequest, HttpResponse, ResponseError};
use futures::future::LocalBoxFuture;
use sqlx::SqlitePool;

use crate::auth::{validate_session, DEFAULT_IDLE_TIMEOUT_SECS, SESSION_TOKEN_KEY};
use crate::error::ApiError;
use crate::models::UserContext;

/// Wrapper error that carries an `ApiError` but implements `ResponseError`
/// so actix-web renders it as a JSON envelope with the correct status code.
#[derive(Debug)]
struct AuthError(ApiError);

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ResponseError for AuthError {
    fn status_code(&self) -> StatusCode {
        self.0.status_code()
    }

    fn error_response(&self) -> HttpResponse {
        self.0.error_response()
    }
}

/// Extract authenticated UserContext from the request.
///
/// This extractor:
/// 1. Reads the session token from the actix-session.
/// 2. Validates it against the database (checks expiry, disabled status).
/// 3. Returns the UserContext on success.
///
/// If no session token is present or the session is invalid, returns a
/// 401 UNAUTHENTICATED (or SESSION_EXPIRED / ACCOUNT_DISABLED) error
/// with the standard JSON error envelope.
impl FromRequest for UserContext {
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let req = req.clone();

        Box::pin(async move {
            // Get the session from the request
            let session = Session::extract(&req).await.map_err(|_| {
                AuthError(ApiError::new(
                    crate::error::codes::UNAUTHENTICATED,
                    "Not authenticated",
                ))
            })?;

            // Get the session token from the actix-session
            let token_opt: Option<String> = session.get(SESSION_TOKEN_KEY).unwrap_or(None);

            let token = match token_opt {
                Some(t) => t,
                None => {
                    return Err(AuthError(ApiError::new(
                        crate::error::codes::UNAUTHENTICATED,
                        "Not authenticated",
                    ))
                    .into());
                }
            };

            // Get the DB pool from app_data
            let pool = req
                .app_data::<actix_web::web::Data<SqlitePool>>()
                .ok_or_else(|| {
                    AuthError(ApiError::new(
                        crate::error::codes::INTERNAL_ERROR,
                        "database pool not available",
                    ))
                })?;

            // Get idle timeout from app_data config, or use default
            let idle_timeout: i64 = match req.app_data::<actix_web::web::Data<i64>>() {
                Some(d) => ***d,
                None => DEFAULT_IDLE_TIMEOUT_SECS,
            };

            // Validate the session
            let user_ctx = validate_session(pool, &token, idle_timeout)
                .await
                .map_err(AuthError)?;

            Ok(user_ctx)
        })
    }
}

/// Optional UserContext extractor — returns `None` for anonymous requests
/// instead of 401. Useful for endpoints that behave differently for
/// authenticated vs. anonymous users.
pub struct OptionalUserContext(pub Option<UserContext>);

impl FromRequest for OptionalUserContext {
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let result = UserContext::from_request(req, payload);

        Box::pin(async move {
            match result.await {
                Ok(ctx) => Ok(OptionalUserContext(Some(ctx))),
                Err(_) => Ok(OptionalUserContext(None)),
            }
        })
    }
}
