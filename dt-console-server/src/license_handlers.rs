//! HTTP handlers for license endpoints.
//!
//! - GET  /api/license         — return the current license status
//! - POST /api/license/activate — activate a license with a code (admin-only)
//!
//! Activation codes are self-contained signed payloads that encode
//! `{sku, max_tasks, expire_at, granted_to, sig}`. The signature is
//! the first 16 hex chars of `SHA256(sku:max_tasks:expire_at:granted_to:SECRET)`.
//! Invalid codes are rejected with 400 INVALID_LICENSE_CODE.

use actix_web::{get, post, web, HttpResponse, ResponseError};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::error::{codes, ApiError};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{
    ActivateLicenseRequest, ActivationPayload, License, LicenseResponse, OperateLog, UserContext,
};
use crate::repositories::license_repository::LicenseRepository;
use crate::repositories::operate_log_repository::OperateLogRepository;
use crate::repositories::task_repository::TaskRepository;

/// Secret key used for activation code signature verification.
/// In production this would be loaded from a secure configuration source.
const ACTIVATION_SECRET: &str = "ape-dts-console-license-secret-2025";

/// Number of days before expiry at which status transitions to "expiring_soon".
const EXPIRING_SOON_THRESHOLD_DAYS: i64 = 30;

/// GET /api/license — return the current license status.
///
/// All authenticated roles can read the license.
/// If no license is active, returns `status: "missing"`.
#[get("/license")]
pub async fn get_license(pool: web::Data<SqlitePool>, _user: UserContext) -> HttpResponse {
    let current = match LicenseRepository::get_current(&pool).await {
        Ok(l) => l,
        Err(_) => {
            return ApiError::new(codes::INTERNAL_ERROR, "license query failed").error_response();
        }
    };

    let current_tasks = match TaskRepository::count(&pool).await {
        Ok(n) => n,
        Err(_) => {
            return ApiError::new(codes::INTERNAL_ERROR, "task count failed").error_response();
        }
    };

    let response = match current {
        Some(lic) => license_to_response(&lic, current_tasks),
        None => LicenseResponse {
            sku: String::new(),
            max_tasks: 0,
            expire_at: None,
            status: "missing".to_string(),
            granted_to: String::new(),
            activated_at: None,
            current_tasks,
        },
    };

    HttpResponse::Ok().json(response)
}

/// POST /api/license/activate — activate a license with a code.
///
/// Admin-only. Operator and viewer receive 403.
/// - Valid code → 200 with the new license payload
/// - Invalid code → 400 INVALID_LICENSE_CODE
/// - Negative max_tasks in code → 400 INVALID_LICENSE_CODE
/// - The submitted code is never echoed back or stored in operate_logs.
#[post("/license/activate")]
pub async fn activate_license(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    body: web::Json<ActivateLicenseRequest>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    // RBAC: admin-only
    if let Err(e) = rbac::require_action(&user, RbacAction::LicenseActivate) {
        return e.error_response();
    }

    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Parse and validate the activation code
    let payload = match parse_activation_code(&body.code) {
        Ok(p) => p,
        Err(e) => {
            // Write failure audit log (code redacted)
            let _ = write_license_audit_log(&pool, &user.username, "failure", &ip, None).await;
            return e.error_response();
        }
    };

    // Validate max_tasks is non-negative
    if payload.max_tasks < 0 {
        let _ = write_license_audit_log(&pool, &user.username, "failure", &ip, None).await;
        return ApiError::new(codes::INVALID_LICENSE_CODE, "Invalid license code").error_response();
    }

    // Verify signature
    if !verify_signature(&payload) {
        let _ = write_license_audit_log(&pool, &user.username, "failure", &ip, None).await;
        return ApiError::new(codes::INVALID_LICENSE_CODE, "Invalid license code").error_response();
    }

    // Build the license row (upsert: if a license exists, update it)
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let code_hash = format!("{:x}", Sha256::digest(body.code.as_bytes()));

    let current = LicenseRepository::get_current(&pool).await.ok().flatten();

    let license = if let Some(mut existing) = current {
        // Update existing license
        existing.sku = payload.sku;
        existing.max_tasks = payload.max_tasks;
        existing.expire_at = Some(payload.expire_at.clone());
        existing.activated_at = Some(now.clone());
        existing.activation_code_hash = Some(code_hash);
        existing.granted_to = payload.granted_to;
        existing.updated_at = now;

        match LicenseRepository::update(&pool, &existing).await {
            Ok(l) => l,
            Err(e) => {
                return ApiError::new(codes::INTERNAL_ERROR, format!("license update failed: {e}"))
                    .error_response();
            }
        }
    } else {
        // Create new license
        let new_license = License {
            id: uuid::Uuid::new_v4().to_string(),
            sku: payload.sku,
            max_tasks: payload.max_tasks,
            expire_at: Some(payload.expire_at),
            activated_at: Some(now.clone()),
            activation_code_hash: Some(code_hash),
            granted_to: payload.granted_to,
            created_at: now.clone(),
            updated_at: now,
        };

        match LicenseRepository::create(&pool, &new_license).await {
            Ok(l) => l,
            Err(e) => {
                return ApiError::new(
                    codes::INTERNAL_ERROR,
                    format!("license creation failed: {e}"),
                )
                .error_response();
            }
        }
    };

    // Audit log (code redacted — never write the raw code)
    let _ = write_license_audit_log(&pool, &user.username, "success", &ip, None).await;

    let current_tasks = TaskRepository::count(&pool).await.unwrap_or(0);

    HttpResponse::Ok().json(license_to_response(&license, current_tasks))
}

/// Decode a base64-encoded activation code into its payload.
///
/// The code is a base64url-encoded JSON string containing:
/// `{ "sku": "...", "maxTasks": N, "expireAt": "...", "grantedTo": "...", "sig": "..." }`
fn parse_activation_code(code: &str) -> Result<ActivationPayload, ApiError> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let bytes = URL_SAFE_NO_PAD
        .decode(code)
        .map_err(|_| ApiError::new(codes::INVALID_LICENSE_CODE, "Invalid license code"))?;

    let payload: ActivationPayload = serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::new(codes::INVALID_LICENSE_CODE, "Invalid license code"))?;

    Ok(payload)
}

/// Generate an activation code from a payload.
/// Available publicly so integration tests can generate codes.
pub fn generate_activation_code(payload: &ActivationPayload) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let json = serde_json::to_string(payload).unwrap();
    URL_SAFE_NO_PAD.encode(json.as_bytes())
}

/// Verify the signature of an activation payload.
///
/// The signature is the first 16 hex chars of
/// `SHA256(sku + ":" + max_tasks + ":" + expire_at + ":" + granted_to + ":" + SECRET)`.
fn verify_signature(payload: &ActivationPayload) -> bool {
    let expected_sig = compute_signature(
        &payload.sku,
        payload.max_tasks,
        &payload.expire_at,
        &payload.granted_to,
    );
    payload.sig == expected_sig
}

/// Compute the signature for an activation code.
pub fn compute_signature(sku: &str, max_tasks: i64, expire_at: &str, granted_to: &str) -> String {
    let message = format!("{sku}:{max_tasks}:{expire_at}:{granted_to}:{ACTIVATION_SECRET}");
    let hash = Sha256::digest(message.as_bytes());
    let hex = format!("{hash:x}");
    hex[..16].to_string()
}

/// Compute the license status based on expiry date.
///
/// - `missing`  — no license row
/// - `expired`  — expire_at is in the past
/// - `expiring_soon` — expire_at is within 30 days
/// - `active`   — expire_at is far in the future
pub fn compute_license_status(expire_at: Option<&str>) -> String {
    let expire_str = match expire_at {
        Some(s) if !s.is_empty() => s,
        _ => return "active".to_string(), // no expiry = perpetual license
    };

    let expire = match chrono::DateTime::parse_from_rfc3339(expire_str) {
        Ok(dt) => dt.with_timezone(&chrono::Utc),
        Err(_) => return "active".to_string(), // unparseable → treat as active
    };

    let now = chrono::Utc::now();

    if now > expire {
        "expired".to_string()
    } else if (expire - now) <= chrono::Duration::days(EXPIRING_SOON_THRESHOLD_DAYS) {
        "expiring_soon".to_string()
    } else {
        "active".to_string()
    }
}

/// Check whether a new task can be created under the current license.
///
/// Returns `Ok(())` if creation is allowed, `Err(ApiError)` if blocked.
///
/// Blocking conditions:
/// - No license row → 422 LICENSE_LIMIT_EXCEEDED (missing license = no capacity)
/// - current_tasks >= max_tasks → 422 LICENSE_LIMIT_EXCEEDED
/// - License is expired → 422 LICENSE_EXPIRED
pub async fn check_license_cap(pool: &SqlitePool) -> Result<License, ApiError> {
    let current = LicenseRepository::get_current(pool)
        .await
        .map_err(|e| ApiError::new(codes::INTERNAL_ERROR, format!("license query failed: {e}")))?
        .ok_or_else(|| {
            ApiError::with_details(
                codes::LICENSE_LIMIT_EXCEEDED,
                "No active license; task creation is not allowed",
                serde_json::json!({ "maxTasks": 0, "currentTasks": 0 }),
            )
        })?;

    let status = compute_license_status(current.expire_at.as_deref());

    if status == "expired" {
        let current_tasks = TaskRepository::count(pool).await.unwrap_or(0);
        return Err(ApiError::with_details(
            codes::LICENSE_EXPIRED,
            "License has expired; task creation is not allowed",
            serde_json::json!({ "status": "expired", "currentTasks": current_tasks }),
        ));
    }

    let current_tasks = TaskRepository::count(pool)
        .await
        .map_err(|e| ApiError::new(codes::INTERNAL_ERROR, format!("task count failed: {e}")))?;

    if current_tasks >= current.max_tasks {
        return Err(ApiError::with_details(
            codes::LICENSE_LIMIT_EXCEEDED,
            "License task limit exceeded",
            serde_json::json!({ "maxTasks": current.max_tasks, "currentTasks": current_tasks }),
        ));
    }

    Ok(current)
}

/// Check whether a task can be started under the current license.
///
/// Returns `Ok(())` if start is allowed, `Err(ApiError)` if blocked.
///
/// Blocking conditions:
/// - License is expired → 422 LICENSE_EXPIRED
/// - No license → 422 LICENSE_EXPIRED
pub async fn check_license_for_start(pool: &SqlitePool) -> Result<(), ApiError> {
    let current = LicenseRepository::get_current(pool)
        .await
        .map_err(|e| ApiError::new(codes::INTERNAL_ERROR, format!("license query failed: {e}")))?
        .ok_or_else(|| {
            ApiError::with_details(
                codes::LICENSE_EXPIRED,
                "No active license; task start is not allowed",
                serde_json::json!({ "status": "missing" }),
            )
        })?;

    let status = compute_license_status(current.expire_at.as_deref());

    if status == "expired" {
        return Err(ApiError::with_details(
            codes::LICENSE_EXPIRED,
            "License has expired; task start is not allowed",
            serde_json::json!({ "status": "expired" }),
        ));
    }

    Ok(())
}

/// Convert a License model to a LicenseResponse DTO.
fn license_to_response(license: &License, current_tasks: i64) -> LicenseResponse {
    let status = compute_license_status(license.expire_at.as_deref());
    LicenseResponse {
        sku: license.sku.clone(),
        max_tasks: license.max_tasks,
        expire_at: license.expire_at.clone(),
        status,
        granted_to: license.granted_to.clone(),
        activated_at: license.activated_at.clone(),
        current_tasks,
    }
}

/// Write an audit log for license activation.
/// The activation code is never included in the audit log.
async fn write_license_audit_log(
    pool: &SqlitePool,
    actor: &str,
    result: &str,
    ip: &str,
    details: Option<&str>,
) -> Result<(), ApiError> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let log = OperateLog {
        id: 0,
        actor: actor.to_string(),
        action: "license.activate".to_string(),
        result: result.to_string(),
        target: None,
        details: details.map(|d| d.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_license_status_missing() {
        assert_eq!(compute_license_status(None), "active");
    }

    #[test]
    fn test_compute_license_status_empty_string() {
        assert_eq!(compute_license_status(Some("")), "active");
    }

    #[test]
    fn test_compute_license_status_expired() {
        let past = "2020-01-01T00:00:00Z";
        assert_eq!(compute_license_status(Some(past)), "expired");
    }

    #[test]
    fn test_compute_license_status_expiring_soon() {
        let soon = chrono::Utc::now() + chrono::Duration::days(7);
        let soon_str = soon.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        assert_eq!(compute_license_status(Some(&soon_str)), "expiring_soon");
    }

    #[test]
    fn test_compute_license_status_active() {
        let far = chrono::Utc::now() + chrono::Duration::days(365);
        let far_str = far.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        assert_eq!(compute_license_status(Some(&far_str)), "active");
    }

    #[test]
    fn test_compute_license_status_just_before_threshold() {
        // 31 days out → still "active"
        let future = chrono::Utc::now() + chrono::Duration::days(31);
        let future_str = future.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        assert_eq!(compute_license_status(Some(&future_str)), "active");
    }

    #[test]
    fn test_compute_license_status_just_inside_threshold() {
        // 29 days out → "expiring_soon"
        let future = chrono::Utc::now() + chrono::Duration::days(29);
        let future_str = future.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        assert_eq!(compute_license_status(Some(&future_str)), "expiring_soon");
    }

    #[test]
    fn test_signature_roundtrip() {
        let sig = compute_signature("pro", 10, "2026-12-31T23:59:59Z", "acme-corp");
        let payload = ActivationPayload {
            sku: "pro".to_string(),
            max_tasks: 10,
            expire_at: "2026-12-31T23:59:59Z".to_string(),
            granted_to: "acme-corp".to_string(),
            sig: sig.clone(),
        };
        assert!(verify_signature(&payload));

        // Tampered signature
        let mut tampered = payload.clone();
        tampered.sig = "deadbeef12345678".to_string();
        assert!(!verify_signature(&tampered));
    }

    #[test]
    fn test_parse_invalid_base64() {
        assert!(parse_activation_code("!!!not-base64!!!").is_err());
    }

    #[test]
    fn test_parse_valid_base64_but_invalid_json() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let code = URL_SAFE_NO_PAD.encode(b"not json");
        assert!(parse_activation_code(&code).is_err());
    }

    #[test]
    fn test_generate_and_parse_activation_code() {
        let sig = compute_signature("enterprise", 100, "2030-01-01T00:00:00Z", "bigcorp");
        let payload = ActivationPayload {
            sku: "enterprise".to_string(),
            max_tasks: 100,
            expire_at: "2030-01-01T00:00:00Z".to_string(),
            granted_to: "bigcorp".to_string(),
            sig,
        };
        let code = generate_activation_code(&payload);
        let parsed = parse_activation_code(&code).unwrap();
        assert_eq!(parsed.sku, "enterprise");
        assert_eq!(parsed.max_tasks, 100);
        assert_eq!(parsed.expire_at, "2030-01-01T00:00:00Z");
        assert_eq!(parsed.granted_to, "bigcorp");
        assert!(verify_signature(&parsed));
    }
}
