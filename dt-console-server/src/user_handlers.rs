//! HTTP handlers for user management endpoints (admin-only).
//!
//! - GET    /api/users      — list all users
//! - POST   /api/users      — create a user
//! - GET    /api/users/:id   — get a single user
//! - PATCH  /api/users/:id   — update a user
//! - DELETE /api/users/:id   — delete a user

use actix_web::{delete, get, patch, post, web, HttpResponse, ResponseError};
use sqlx::SqlitePool;

use crate::auth;
use crate::error::{codes, ApiError};
use crate::models::OperateLog;
use crate::models::{CreateUserRequest, UpdateUserRequest, UserContext, UserResponse};
use crate::repositories::operate_log_repository::OperateLogRepository;
use crate::repositories::user_repository::UserRepository;

/// Convert a User model to a UserResponse (strips password_hash, updated_at).
fn user_to_response(user: &crate::models::User) -> UserResponse {
    UserResponse {
        id: user.id.clone(),
        username: user.username.clone(),
        display_name: user.display_name.clone(),
        role: user.role.clone(),
        disabled: user.disabled,
        created_at: user.created_at.clone(),
    }
}

/// Write an audit log for user management actions.
async fn write_user_audit_log(
    pool: &SqlitePool,
    actor: &str,
    action: &str,
    result: &str,
    target: &str,
    ip: &str,
) -> Result<(), ApiError> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let log = OperateLog {
        id: 0,
        actor: actor.to_string(),
        action: action.to_string(),
        result: result.to_string(),
        target: Some(target.to_string()),
        details: None,
        ip: Some(ip.to_string()),
        created_at: now,
    };
    OperateLogRepository::create(pool, &log)
        .await
        .map_err(|e| {
            ApiError::new(
                codes::INTERNAL_ERROR,
                format!("audit log write failed: {e}"),
            )
        })?;
    Ok(())
}

/// Write an audit log with details for user management actions.
async fn write_user_audit_log_with_details(
    pool: &SqlitePool,
    actor: &str,
    action: &str,
    result: &str,
    target: &str,
    ip: &str,
    details: &str,
) -> Result<(), ApiError> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let log = OperateLog {
        id: 0,
        actor: actor.to_string(),
        action: action.to_string(),
        result: result.to_string(),
        target: Some(target.to_string()),
        details: Some(details.to_string()),
        ip: Some(ip.to_string()),
        created_at: now,
    };
    OperateLogRepository::create(pool, &log)
        .await
        .map_err(|e| {
            ApiError::new(
                codes::INTERNAL_ERROR,
                format!("audit log write failed: {e}"),
            )
        })?;
    Ok(())
}

/// GET /api/users — list all users.
///
/// Admin-only. Returns array of UserResponse (no password fields).
#[get("/users")]
pub async fn list_users(pool: web::Data<SqlitePool>, user: UserContext) -> HttpResponse {
    // Admin-only check
    if user.role != "admin" {
        return ApiError::with_details(
            codes::FORBIDDEN,
            "Insufficient permissions",
            serde_json::json!({ "required_action": "users.list" }),
        )
        .error_response();
    }

    let users = match UserRepository::list(&pool).await {
        Ok(u) => u,
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("user list failed: {e}"))
                .error_response();
        }
    };

    let responses: Vec<UserResponse> = users.iter().map(user_to_response).collect();
    HttpResponse::Ok().json(responses)
}

/// POST /api/users — create a user.
///
/// Admin-only. Duplicate username → 409 USERNAME_TAKEN.
/// Password is bcrypt-hashed before storage.
#[post("/users")]
pub async fn create_user(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    body: web::Json<CreateUserRequest>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    // Admin-only check
    if user.role != "admin" {
        return ApiError::with_details(
            codes::FORBIDDEN,
            "Insufficient permissions",
            serde_json::json!({ "required_action": "users.create" }),
        )
        .error_response();
    }

    // Validate role value
    if !["admin", "operator", "viewer"].contains(&body.role.as_str()) {
        return ApiError::new(codes::VALIDATION_FAILED, "Invalid role value").error_response();
    }

    // Extract IP early for audit logging
    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Check for duplicate username
    if UserRepository::find_by_username(&pool, &body.username)
        .await
        .is_ok()
    {
        // Write failure audit log
        let _ = write_user_audit_log_with_details(
            &pool,
            &user.username,
            "users.create",
            "failure",
            &body.username,
            &ip,
            r#"{"reason":"duplicate_username"}"#,
        )
        .await;

        return ApiError::new(codes::USERNAME_TAKEN, "Username already exists").error_response();
    }

    // Hash password
    let password_hash = match auth::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => return e.error_response(),
    };

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let id = uuid::Uuid::new_v4().to_string();
    let display_name = if body.display_name.is_empty() {
        body.username.clone()
    } else {
        body.display_name.clone()
    };

    let new_user = crate::models::User {
        id: id.clone(),
        username: body.username.clone(),
        password_hash,
        display_name,
        role: body.role.clone(),
        disabled: false,
        created_at: now.clone(),
        updated_at: now,
    };

    let saved = match UserRepository::create(&pool, &new_user).await {
        Ok(u) => u,
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("user creation failed: {e}"))
                .error_response();
        }
    };

    // Audit log
    let _ = write_user_audit_log(&pool, &user.username, "users.create", "success", &id, &ip).await;

    HttpResponse::Created().json(user_to_response(&saved))
}

/// GET /api/users/:id — get a single user.
///
/// Admin-only. Returns 404 if user not found.
#[get("/users/{id}")]
pub async fn get_user(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    // Admin-only check
    if user.role != "admin" {
        return ApiError::with_details(
            codes::FORBIDDEN,
            "Insufficient permissions",
            serde_json::json!({ "required_action": "users.read" }),
        )
        .error_response();
    }

    let id = path.into_inner();
    let found = match UserRepository::find_by_id(&pool, &id).await {
        Ok(u) => u,
        Err(_) => {
            return ApiError::new(codes::NOT_FOUND, "User not found").error_response();
        }
    };

    HttpResponse::Ok().json(user_to_response(&found))
}

/// PATCH /api/users/:id — update a user.
///
/// Admin-only, with self-PATCH guards:
/// - Cannot promote own role (self-PATCH with role=admin → 403)
/// - Cannot demote self (admin demoting themselves → 422)
///
/// Password reset → bcrypt rehash + session invalidation.
/// Disable user → session invalidation.
#[patch("/users/{id}")]
pub async fn update_user(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    body: web::Json<UpdateUserRequest>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    // Admin-only check
    if user.role != "admin" {
        return ApiError::with_details(
            codes::FORBIDDEN,
            "Insufficient permissions",
            serde_json::json!({ "required_action": "users.update" }),
        )
        .error_response();
    }

    let id = path.into_inner();
    let mut found = match UserRepository::find_by_id(&pool, &id).await {
        Ok(u) => u,
        Err(_) => {
            return ApiError::new(codes::NOT_FOUND, "User not found").error_response();
        }
    };

    // Self-PATCH guard: admin cannot demote themselves (would violate last-admin invariant)
    if user.user_id == id {
        if let Some(ref new_role) = body.role {
            if new_role != "admin" {
                return ApiError::new(
                    codes::CANNOT_DEMOTE_SELF,
                    "Cannot demote yourself; would violate last-admin invariant",
                )
                .error_response();
            }
        }
    }

    // Self-PATCH guard: cannot change own role (e.g. promote self)
    if user.user_id == id {
        if let Some(ref new_role) = body.role {
            // Changing role on self to a different role is forbidden
            // (keeping same role is allowed for other updates)
            if new_role != &user.role {
                return ApiError::with_details(
                    codes::FORBIDDEN,
                    "Cannot change your own role",
                    serde_json::json!({ "required_action": "users.update_role" }),
                )
                .error_response();
            }
        }
    }

    // Apply password update (bcrypt rehash + session invalidation)
    if let Some(ref new_password) = body.password {
        found.password_hash = match auth::hash_password(new_password) {
            Ok(h) => h,
            Err(e) => return e.error_response(),
        };
        // Invalidate all sessions for the user
        if let Err(e) = auth::invalidate_user_sessions(&pool, &id).await {
            return e.error_response();
        }
    }

    // Apply role update
    if let Some(ref new_role) = body.role {
        if !["admin", "operator", "viewer"].contains(&new_role.as_str()) {
            return ApiError::new(codes::VALIDATION_FAILED, "Invalid role value").error_response();
        }
        found.role = new_role.clone();
    }

    // Apply display name update
    if let Some(ref display_name) = body.display_name {
        found.display_name = display_name.clone();
    }

    // Apply disabled toggle
    if let Some(disabled) = body.disabled {
        found.disabled = disabled;
        // When disabling, sessions are NOT deleted — they will be rejected
        // on next request because validate_session checks the disabled flag.
        // This ensures the error code is ACCOUNT_DISABLED, not UNAUTHENTICATED.
        // When re-enabling, existing sessions naturally resume working.
    }

    found.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let saved = match UserRepository::update(&pool, &found).await {
        Ok(u) => u,
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("user update failed: {e}"))
                .error_response();
        }
    };

    // Audit log
    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let _ = write_user_audit_log(&pool, &user.username, "users.update", "success", &id, &ip).await;

    HttpResponse::Ok().json(user_to_response(&saved))
}

/// DELETE /api/users/:id — delete a user.
///
/// Admin-only. Cannot delete the last admin → 409.
/// Deletion cascades: all sessions for the user are invalidated.
/// Deleted user cannot login.
#[delete("/users/{id}")]
pub async fn delete_user(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    // Admin-only check
    if user.role != "admin" {
        return ApiError::with_details(
            codes::FORBIDDEN,
            "Insufficient permissions",
            serde_json::json!({ "required_action": "users.delete" }),
        )
        .error_response();
    }

    let id = path.into_inner();

    // Audit log before deletion (need user info for target)
    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Perform deletion (includes last-admin check + session cascade)
    if let Err(e) = auth::delete_user(&pool, &id).await {
        return e.error_response();
    }

    // Audit log
    let _ = write_user_audit_log(&pool, &user.username, "users.delete", "success", &id, &ip).await;

    HttpResponse::NoContent().finish()
}
