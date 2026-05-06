//! HTTP handlers for authentication endpoints.
//!
//! - POST /api/auth/login — verify credentials, create session
//! - POST /api/auth/logout — invalidate session
//! - GET  /api/auth/me — return current user info

use actix_session::Session;
use actix_web::{get, post, web, HttpResponse, ResponseError};
use sqlx::SqlitePool;

use crate::auth::{self, LoginResult, DEFAULT_IDLE_TIMEOUT_SECS, SESSION_TOKEN_KEY};
use crate::error::{codes, ApiError};
use crate::models::{AuthResponse, LoginRequest, UserContext};
use crate::rate_limit::RateLimiter;

/// POST /api/auth/login
///
/// Request body: `{ "username": "...", "password": "..." }`
///
/// On success: 200 with `{ "username", "displayName", "role" }` and
/// Set-Cookie with session token stored in the actix-session.
///
/// On failure: 401 (INVALID_CREDENTIALS / ACCOUNT_DISABLED) or 429 (rate-limited).
#[post("/auth/login")]
pub async fn login(
    pool: web::Data<SqlitePool>,
    rate_limiter: web::Data<RateLimiter>,
    session: Session,
    body: web::Json<LoginRequest>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Get idle timeout from app_data, or use default
    let idle_timeout: i64 = match req.app_data::<web::Data<i64>>() {
        Some(d) => ***d,
        None => DEFAULT_IDLE_TIMEOUT_SECS,
    };

    // If there's a pre-existing session token, invalidate it (session rotation)
    if let Ok(Some(old_token)) = session.get::<String>(SESSION_TOKEN_KEY) {
        let _ = auth::logout(&pool, &old_token).await;
    }

    // Attempt login
    let result = auth::login(
        &pool,
        &rate_limiter,
        &body.username,
        &body.password,
        &ip,
        idle_timeout,
    )
    .await;

    match result {
        Ok(LoginResult {
            user,
            session_token,
        }) => {
            // Store the new session token in the actix-session
            if let Err(e) = session.insert(SESSION_TOKEN_KEY, &session_token) {
                tracing::error!("failed to store session token: {e}");
                return HttpResponse::InternalServerError().json(ApiError::new(
                    codes::INTERNAL_ERROR,
                    "session storage failed",
                ));
            }

            HttpResponse::Ok().json(AuthResponse {
                username: user.username,
                display_name: user.display_name,
                role: user.role,
            })
        }
        Err(e) => e.error_response(),
    }
}

/// POST /api/auth/logout
///
/// Invalidates the current session server-side and clears the actix-session.
///
/// On success: 200
/// On failure (no session): 200 (idempotent — logout is always "successful")
#[post("/auth/logout")]
pub async fn logout(
    pool: web::Data<SqlitePool>,
    session: Session,
    _user: UserContext,
) -> HttpResponse {
    // Get the session token from actix-session
    if let Ok(Some(token)) = session.get::<String>(SESSION_TOKEN_KEY) {
        let _ = auth::logout(&pool, &token).await;
    }

    // Clear the actix-session data
    session.clear();

    HttpResponse::Ok().json(serde_json::json!({ "status": "ok" }))
}

/// GET /api/auth/me
///
/// Returns the current authenticated user's info.
///
/// On success: 200 with `{ "username", "displayName", "role" }`
/// On failure (no session): 401 UNAUTHENTICATED
#[get("/auth/me")]
pub async fn me(user: UserContext) -> HttpResponse {
    HttpResponse::Ok().json(AuthResponse {
        username: user.username,
        display_name: user.display_name,
        role: user.role,
    })
}
