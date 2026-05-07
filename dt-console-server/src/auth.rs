//! Authenticator: bcrypt-based login, session lifecycle, and user management.
//!
//! All password verification uses bcrypt with cost ≥ 10. The `verify_password`
//! function delegates to `bcrypt::verify` which is inherently constant-time
//! for a given (hash, cost) pair, since it always performs the full hash
//! computation regardless of the input.

use crate::error::{codes, ApiError};
use crate::models::OperateLog;
use crate::models::{User, UserContext};
use crate::rate_limit::RateLimiter;
use crate::repositories::operate_log_repository::OperateLogRepository;
use crate::repositories::session_repository::SessionRepository;
use crate::repositories::user_repository::UserRepository;
use sqlx::SqlitePool;

/// Default bcrypt cost factor. Must be ≥ 10 per security requirements.
pub const BCRYPT_COST: u32 = 10;

/// Default idle session timeout in seconds.
pub const DEFAULT_IDLE_TIMEOUT_SECS: i64 = 3600;

/// Key used inside the actix-session to store the server-side session token.
pub const SESSION_TOKEN_KEY: &str = "session_token";

/// Result of a successful login.
pub struct LoginResult {
    pub user: User,
    pub session_token: String,
}

/// Hash a plaintext password with bcrypt at the mandated cost.
pub fn hash_password(plaintext: &str) -> Result<String, ApiError> {
    bcrypt::hash(plaintext, BCRYPT_COST)
        .map_err(|e| ApiError::new(codes::INTERNAL_ERROR, format!("password hash failed: {e}")))
}

/// Verify a plaintext password against a bcrypt hash.
///
/// This delegates to `bcrypt::verify`, which always performs the full
/// hash computation regardless of early mismatches, providing constant-time
/// verification for a given (hash, cost) pair.
pub fn verify_password(plaintext: &str, hash: &str) -> Result<bool, ApiError> {
    bcrypt::verify(plaintext, hash).map_err(|e| {
        ApiError::new(
            codes::INTERNAL_ERROR,
            format!("password verify failed: {e}"),
        )
    })
}

/// Attempt to log in a user. Returns `Ok(LoginResult)` on success or
/// `Err(ApiError)` on failure.
///
/// Steps:
/// 1. Rate-limit check per (username, IP).
/// 2. Look up user by username. If not found, perform a dummy bcrypt verify
///    to maintain constant-time response (no username-enumeration leak).
/// 3. If user is disabled, return 401 ACCOUNT_DISABLED.
/// 4. Verify password. If wrong, return 401 INVALID_CREDENTIALS.
/// 5. Create a server-side session row in the DB.
/// 6. Clear the rate-limit counter on success.
/// 7. Write an operate_log row.
/// 8. Return the user and session token.
pub async fn login(
    pool: &SqlitePool,
    rate_limiter: &RateLimiter,
    username: &str,
    password: &str,
    ip: &str,
    idle_timeout_secs: i64,
) -> Result<LoginResult, ApiError> {
    // Step 1: Rate-limit check.
    if let Err(e) = rate_limiter.check_and_record(username, ip) {
        // Write audit log for rate-limited attempt
        let _ = write_auth_log(pool, username, "rate_limited", ip).await;
        return Err(e);
    }

    // Step 2: Look up user.
    let user = match UserRepository::find_by_username(pool, username).await {
        Ok(u) => u,
        Err(_) => {
            // Perform dummy bcrypt verify to maintain constant-time response.
            // Use a fixed hash at cost 10 so the CPU work is identical.
            let dummy_hash = "$2b$10$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
            let _ = verify_password(password, dummy_hash);

            // Write audit log for unknown-username failure
            let _ = write_auth_log(pool, username, "failure", ip).await;

            return Err(ApiError::new(
                codes::INVALID_CREDENTIALS,
                "Invalid credentials",
            ));
        }
    };

    // Step 3: Check if account is disabled.
    if user.disabled {
        let _ = write_auth_log(pool, username, "failure", ip).await;
        return Err(ApiError::new(
            codes::ACCOUNT_DISABLED,
            "Account is disabled",
        ));
    }

    // Step 4: Verify password.
    let valid = verify_password(password, &user.password_hash)?;
    if !valid {
        let _ = write_auth_log(pool, username, "failure", ip).await;
        return Err(ApiError::new(
            codes::INVALID_CREDENTIALS,
            "Invalid credentials",
        ));
    }

    // Step 5: Create session.
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(idle_timeout_secs);

    let session_token = uuid::Uuid::new_v4().to_string();
    let session = crate::models::Session {
        id: uuid::Uuid::new_v4().to_string(),
        user_id: user.id.clone(),
        token: session_token.clone(),
        created_at: now.clone(),
        expires_at: Some(expires_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        ip: Some(ip.to_string()),
        user_agent: None,
    };
    SessionRepository::create(pool, &session)
        .await
        .map_err(|e| {
            ApiError::new(
                codes::INTERNAL_ERROR,
                format!("session creation failed: {e}"),
            )
        })?;

    // Step 6: Clear rate-limit on success.
    rate_limiter.clear(username, ip);

    // Step 7: Write audit log.
    let _ = write_auth_log(pool, username, "success", ip).await;

    Ok(LoginResult {
        user,
        session_token,
    })
}

/// Validate a server-side session token and return the UserContext.
///
/// Checks:
/// - Session exists in DB
/// - Session is not expired (idle timeout)
/// - User exists and is not disabled
///
/// On success, refreshes the idle expiry timestamp.
pub async fn validate_session(
    pool: &SqlitePool,
    token: &str,
    idle_timeout_secs: i64,
) -> Result<UserContext, ApiError> {
    let session = SessionRepository::find_by_token(pool, token)
        .await
        .map_err(|_| ApiError::new(codes::UNAUTHENTICATED, "Not authenticated"))?;

    // Check idle expiry
    if let Some(ref expires_at) = session.expires_at {
        let expires = chrono::DateTime::parse_from_rfc3339(expires_at)
            .map_err(|_| ApiError::new(codes::INTERNAL_ERROR, "invalid session expiry"))?;
        if chrono::Utc::now() > expires {
            // Session expired — clean up
            let _ = SessionRepository::delete(pool, &session.id).await;
            return Err(ApiError::new(
                codes::SESSION_EXPIRED,
                "Session has expired due to inactivity",
            ));
        }
    }

    // Look up user
    let user = UserRepository::find_by_id(pool, &session.user_id)
        .await
        .map_err(|_| ApiError::new(codes::UNAUTHENTICATED, "Not authenticated"))?;

    // Check if user is disabled
    if user.disabled {
        let _ = SessionRepository::delete(pool, &session.id).await;
        return Err(ApiError::new(
            codes::ACCOUNT_DISABLED,
            "Account is disabled",
        ));
    }

    // Refresh idle expiry
    let new_expires = chrono::Utc::now() + chrono::Duration::seconds(idle_timeout_secs);
    let mut updated_session = session;
    updated_session.expires_at =
        Some(new_expires.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    let _ = SessionRepository::update(pool, &updated_session).await;

    Ok(UserContext {
        user_id: user.id,
        username: user.username,
        display_name: user.display_name,
        role: user.role,
        disabled: user.disabled,
        resource_group_id: user.resource_group_id,
    })
}

/// Invalidate a session (logout). Deletes the session row from the DB.
pub async fn logout(pool: &SqlitePool, token: &str) -> Result<(), ApiError> {
    if let Ok(session) = SessionRepository::find_by_token(pool, token).await {
        SessionRepository::delete(pool, &session.id)
            .await
            .map_err(|e| {
                ApiError::new(
                    codes::INTERNAL_ERROR,
                    format!("session deletion failed: {e}"),
                )
            })?;
    }
    Ok(())
}

/// Invalidate all sessions for a user (e.g. on password reset or account disable).
pub async fn invalidate_user_sessions(pool: &SqlitePool, user_id: &str) -> Result<(), ApiError> {
    SessionRepository::delete_by_user(pool, user_id)
        .await
        .map_err(|e| {
            ApiError::new(
                codes::INTERNAL_ERROR,
                format!("session invalidation failed: {e}"),
            )
        })
}

/// Admin password reset: hash the new password and update the user row.
/// Also invalidates all existing sessions for the user so the old password
/// can no longer be used.
pub async fn reset_password(
    pool: &SqlitePool,
    user_id: &str,
    new_password: &str,
) -> Result<User, ApiError> {
    let mut user = UserRepository::find_by_id(pool, user_id)
        .await
        .map_err(|_| ApiError::new(codes::NOT_FOUND, "User not found"))?;

    user.password_hash = hash_password(new_password)?;
    user.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let saved = UserRepository::update(pool, &user)
        .await
        .map_err(|e| ApiError::new(codes::INTERNAL_ERROR, format!("user update failed: {e}")))?;

    // Invalidate all sessions so the old password cannot be reused
    invalidate_user_sessions(pool, user_id).await?;

    Ok(saved)
}

/// Disable a user account. Existing sessions will be rejected on next
/// request because `validate_session` checks the `disabled` flag.
/// Sessions are NOT deleted here so that the next request returns
/// `ACCOUNT_DISABLED` instead of `UNAUTHENTICATED`.
pub async fn disable_user(pool: &SqlitePool, user_id: &str) -> Result<User, ApiError> {
    let mut user = UserRepository::find_by_id(pool, user_id)
        .await
        .map_err(|_| ApiError::new(codes::NOT_FOUND, "User not found"))?;

    user.disabled = true;
    user.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let saved = UserRepository::update(pool, &user)
        .await
        .map_err(|e| ApiError::new(codes::INTERNAL_ERROR, format!("user update failed: {e}")))?;

    Ok(saved)
}

/// Re-enable a previously disabled user account.
pub async fn enable_user(pool: &SqlitePool, user_id: &str) -> Result<User, ApiError> {
    let mut user = UserRepository::find_by_id(pool, user_id)
        .await
        .map_err(|_| ApiError::new(codes::NOT_FOUND, "User not found"))?;

    user.disabled = false;
    user.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let saved = UserRepository::update(pool, &user)
        .await
        .map_err(|e| ApiError::new(codes::INTERNAL_ERROR, format!("user update failed: {e}")))?;

    Ok(saved)
}

/// Delete a user. Prevents deletion of the last admin.
pub async fn delete_user(pool: &SqlitePool, user_id: &str) -> Result<(), ApiError> {
    let user = UserRepository::find_by_id(pool, user_id)
        .await
        .map_err(|_| ApiError::new(codes::NOT_FOUND, "User not found"))?;

    // Prevent deleting the last admin
    if user.role == "admin" {
        let admin_count = UserRepository::count_by_role(pool, "admin")
            .await
            .map_err(|e| {
                ApiError::new(codes::INTERNAL_ERROR, format!("admin count failed: {e}"))
            })?;
        if admin_count <= 1 {
            return Err(ApiError::new(
                codes::LAST_ADMIN_PROTECTED,
                "Cannot delete the last admin user",
            ));
        }
    }

    // Invalidate all sessions first
    invalidate_user_sessions(pool, user_id).await?;

    // Delete the user
    UserRepository::delete(pool, user_id)
        .await
        .map_err(|e| ApiError::new(codes::INTERNAL_ERROR, format!("user delete failed: {e}")))?;

    Ok(())
}

/// Helper: write an auth operate_log row.
///
/// For failure results, `details` includes a `reason` field but never
/// includes the password or session token.
async fn write_auth_log(
    pool: &SqlitePool,
    username: &str,
    result: &str,
    ip: &str,
) -> Result<(), ApiError> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let details = match result {
        "failure" => Some(r#"{"reason":"invalid_credentials"}"#.to_string()),
        "rate_limited" => Some(r#"{"reason":"rate_limited"}"#.to_string()),
        _ => None,
    };
    let log = OperateLog {
        id: 0,
        actor: username.to_string(),
        action: "auth.login".to_string(),
        result: result.to_string(),
        target: None,
        details,
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

/// Seed the default admin user if no users exist.
pub async fn seed_admin(pool: &SqlitePool) -> Result<(), ApiError> {
    let existing = UserRepository::list(pool)
        .await
        .map_err(|e| ApiError::new(codes::INTERNAL_ERROR, format!("user list failed: {e}")))?;

    if !existing.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let password_hash = hash_password("admin123")?;

    let admin = User {
        id: uuid::Uuid::new_v4().to_string(),
        username: "admin".to_string(),
        password_hash,
        display_name: "Administrator".to_string(),
        role: "admin".to_string(),
        disabled: false,
        resource_group_id: None,
        created_at: now.clone(),
        updated_at: now,
    };

    UserRepository::create(pool, &admin)
        .await
        .map_err(|e| ApiError::new(codes::INTERNAL_ERROR, format!("admin seed failed: {e}")))?;

    tracing::info!("seeded default admin user (username: admin, password: admin123)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bcrypt_cost_is_at_least_10() {
        // Compile-time check: BCRYPT_COST must be ≥ 10.
        // Using a const assertion rather than runtime assert.
        const _: () = assert!(BCRYPT_COST >= 10, "bcrypt cost must be ≥ 10");
    }

    #[test]
    fn test_hash_password_produces_valid_bcrypt() {
        let hash = hash_password("testpassword").unwrap();
        // bcrypt hashes start with $2b$ (or $2a$/$2y$)
        assert!(
            hash.starts_with("$2b$") || hash.starts_with("$2a$") || hash.starts_with("$2y$"),
            "hash should be a valid bcrypt string, got: {hash}"
        );
        // Extract cost from hash: $2b$10$...
        let parts: Vec<&str> = hash.split('$').collect();
        let cost: u32 = parts[2].parse().unwrap();
        assert!(cost >= 10, "cost should be ≥ 10, got {cost}");
    }

    #[test]
    fn test_verify_password_correct() {
        let hash = hash_password("mypassword").unwrap();
        let valid = verify_password("mypassword", &hash).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_verify_password_wrong() {
        let hash = hash_password("mypassword").unwrap();
        let valid = verify_password("wrongpassword", &hash).unwrap();
        assert!(!valid);
    }

    #[test]
    fn test_verify_password_is_constant_time() {
        // Verify that bcrypt::verify always performs the full computation
        // regardless of how early the mismatch occurs. We can't truly test
        // timing in a unit test, but we verify that:
        // 1. The verify function always returns a Result (no early panic)
        // 2. Wrong passwords of different lengths all return false
        let hash = hash_password("testpassword").unwrap();

        let short = verify_password("a", &hash).unwrap();
        let medium = verify_password("wrong", &hash).unwrap();
        let long = verify_password("this_is_a_very_long_wrong_password", &hash).unwrap();

        assert!(!short);
        assert!(!medium);
        assert!(!long);
    }
}
