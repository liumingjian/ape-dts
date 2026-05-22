//! HTTP handlers for the operate_log audit endpoint.
//!
//! - GET /api/operate_logs — list audit log entries (admin-only)
//!
//! Operate log rows are immutable: no POST/PATCH/DELETE route exists.
//! Operator and viewer receive 403 FORBIDDEN on GET /api/operate_logs.
//! Activation codes and passwords are redacted in all log/response surfaces.

use actix_web::{get, web, HttpResponse, ResponseError};
use sqlx::SqlitePool;

use crate::error::{codes, ApiError};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{OperateLog, UserContext};
use crate::repositories::operate_log_repository::OperateLogRepository;

/// Response shape for GET /api/operate_logs.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperateLogListResponse {
    pub items: Vec<OperateLogItem>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

/// Single operate_log item in the API response.
/// Field names use camelCase for frontend convention.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperateLogItem {
    pub id: i64,
    pub ts: String,
    pub actor: String,
    pub action: String,
    pub result: String,
    pub target: Option<String>,
    pub details: Option<String>,
    pub ip: Option<String>,
}

/// Query parameters for GET /api/operate_logs.
#[derive(serde::Deserialize, Debug, Default)]
pub struct OperateLogQuery {
    pub actor: Option<String>,
    pub action: Option<String>,
    pub result: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

/// Redact an activation code in a JSON details string.
///
/// Replaces the value of the `"code"` key with `"<redacted>"`.
pub fn redact_activation_code(details: &str) -> String {
    // Match "code":"<any value>" and replace the value with <redacted>
    // This handles the common JSON pattern for activation codes.
    let re = regex::Regex::new(r#""code"\s*:\s*"[^"]*""#).unwrap();
    re.replace_all(details, r#""code":"<redacted>""#)
        .to_string()
}

/// Redact passwords in connection strings.
///
/// Replaces the password portion of URI-style connection strings with `****`.
pub fn redact_connection_string_passwords(text: &str) -> String {
    // Match URI-style connection strings (user:password@host)
    let re = regex::Regex::new(r"(://[^:]+:)([^@]+)(@)").unwrap();
    re.replace_all(text, "${1}****${3}").to_string()
}

/// Redact all sensitive fields in a details string.
fn redact_details(details: &str) -> String {
    let result = redact_activation_code(details);
    redact_connection_string_passwords(&result)
}

/// Convert a model OperateLog to an API OperateLogItem with redaction.
fn log_to_item(log: &OperateLog) -> OperateLogItem {
    OperateLogItem {
        id: log.id,
        ts: log.created_at.clone(),
        actor: log.actor.clone(),
        action: log.action.clone(),
        result: log.result.clone(),
        target: log.target.clone(),
        details: log.details.as_ref().map(|d| redact_details(d)),
        ip: log.ip.clone(),
    }
}

/// GET /api/operate_logs — list audit log entries.
///
/// Admin-only. Supports filters: actor, action, result, from, to.
/// Supports pagination: page, page_size (defaults: 1, 20).
/// Rows ordered by created_at DESC.
#[get("/operate_logs")]
pub async fn list_operate_logs(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    query: web::Query<OperateLogQuery>,
) -> HttpResponse {
    // RBAC: admin-only
    if let Err(e) = rbac::require_action(&user, RbacAction::OperateLogsList) {
        return e.error_response();
    }

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);

    let result = OperateLogRepository::list_filtered(
        &pool,
        query.actor.as_deref(),
        query.action.as_deref(),
        query.result.as_deref(),
        query.from.as_deref(),
        query.to.as_deref(),
        page,
        page_size,
    )
    .await;

    match result {
        Ok((logs, total)) => {
            let items: Vec<OperateLogItem> = logs.iter().map(log_to_item).collect();
            HttpResponse::Ok().json(OperateLogListResponse {
                items,
                total,
                page,
                page_size,
            })
        }
        Err(e) => ApiError::new(
            codes::INTERNAL_ERROR,
            format!("operate_logs query failed: {e}"),
        )
        .error_response(),
    }
}
