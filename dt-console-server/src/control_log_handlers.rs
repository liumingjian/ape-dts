//! HTTP handler for GET /api/control_logs.
//!
//! Returns control log rows ordered by ts (created_at) descending,
//! with optional filters for task_id, action, from, to, run_id.

use actix_web::{get, web, HttpResponse, ResponseError};

use crate::error::{codes, ApiError};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{ControlLogListResponse, UserContext};
use crate::repositories::control_log_repository::{ControlLogFilter, ControlLogRepository};

const DEFAULT_PAGE: i64 = 1;
const DEFAULT_PAGE_SIZE: i64 = 50;

/// GET /api/control_logs — list control logs with optional filters.
///
/// Query parameters:
/// - `task_id`: filter by task ID
/// - `action`: filter by action (start, stop, pause, resume, run_exit)
/// - `from`: filter by created_at >= from (ISO-8601)
/// - `to`: filter by created_at <= to (ISO-8601)
/// - `run_id`: filter by run ID
/// - `page`: page number (default 1)
/// - `page_size`: page size (default 50)
///
/// Admin-only. Ordered by created_at DESC.
#[get("/control_logs")]
pub async fn list_control_logs(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    query: web::Query<ControlLogQuery>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::ControlLogsList) {
        return e.error_response();
    }

    let page = query.page.unwrap_or(DEFAULT_PAGE);
    let page_size = query.page_size.unwrap_or(DEFAULT_PAGE_SIZE);

    let filter = ControlLogFilter {
        task_id: query.task_id.as_deref(),
        action: query.action.as_deref(),
        from: query.from.as_deref(),
        to: query.to.as_deref(),
        run_id: query.run_id.as_deref(),
        page,
        page_size,
    };

    let result = ControlLogRepository::list_filtered(&pool, &filter).await;

    match result {
        Ok((logs, total)) => {
            let items: Vec<_> = logs.iter().map(|l| l.to_response()).collect();
            HttpResponse::Ok().json(ControlLogListResponse {
                items,
                total,
                page,
                page_size,
            })
        }
        Err(e) => ApiError::new(
            codes::INTERNAL_ERROR,
            format!("control log query failed: {e}"),
        )
        .error_response(),
    }
}

/// Query parameters for GET /api/control_logs.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlLogQuery {
    pub task_id: Option<String>,
    pub action: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub run_id: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

/// Finalise orphaned control-log intents.
///
/// Called on orchestrator startup. Finds all intent rows without a
/// corresponding result row and writes a synthetic
/// `result:orphaned_by_restart` row for each.
pub async fn finalise_orphaned_intents(pool: &sqlx::SqlitePool) {
    match ControlLogRepository::finalise_orphaned_intents(pool).await {
        Ok(count) => {
            if count > 0 {
                tracing::info!("finalised {count} orphaned control log intent(s)");
            }
        }
        Err(e) => {
            tracing::warn!("failed to finalise orphaned control log intents: {e}");
        }
    }
}
