//! HTTP handlers for Task CRUD endpoints.
//!
//! - POST   /api/tasks          — create a task
//! - GET    /api/tasks          — list tasks (with filters)
//! - GET    /api/tasks/:id      — get a single task
//! - PATCH  /api/tasks/:id      — update a task
//! - DELETE /api/tasks/:id      — delete a task

use actix_web::http::StatusCode;
use actix_web::{delete, get, patch, post, web, HttpResponse, ResponseError};
use sqlx::SqlitePool;

use crate::error::{codes, ApiError};
use crate::license_handlers::check_license_cap;
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{
    CreateTaskRequest, OperateLog, TaskListResponse, TaskResponse, UpdateTaskRequest, UserContext,
};
use crate::repositories::operate_log_repository::OperateLogRepository;
use crate::repositories::resource_group_repository::ResourceGroupRepository;
use crate::repositories::run_repository::RunRepository;
use crate::repositories::task_repository::TaskRepository;
use crate::validation::{self, ValidationError};

/// Convert a Task model to a TaskResponse DTO.
fn task_to_response(task: &crate::models::Task) -> TaskResponse {
    TaskResponse {
        id: task.id.clone(),
        task_id: task.task_id.clone(),
        name: task.name.clone(),
        kind: task.kind.clone(),
        db_type_source: task.db_type_source.clone(),
        db_type_target: task.db_type_target.clone(),
        source_endpoint: parse_json_or_default(&task.source_endpoint),
        target_endpoint: parse_json_or_default(&task.target_endpoint),
        extractor: parse_json_or_default(&task.extractor_config),
        sinker: parse_json_or_default(&task.sinker_config),
        filter: parse_json_or_default(&task.filter_config),
        router: parse_json_or_default(&task.router_config),
        parallelizer: parse_json_or_default(&task.parallelizer_config),
        pipeline: parse_json_or_default(&task.pipeline_config),
        resumer: parse_json_or_default(&task.resumer_config),
        processor: parse_json_or_default(&task.processor_config),
        runtime: parse_json_or_default(&task.runtime_config),
        metrics: parse_json_or_default(&task.metrics_config),
        resource_group_id: task.resource_group_id.clone(),
        owner_user_id: task.owner_user_id.clone(),
        status: task.status.clone(),
        created_at: task.created_at.clone(),
        updated_at: task.updated_at.clone(),
    }
}

/// Parse a JSON string or return empty object.
fn parse_json_or_default(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or(serde_json::Value::Object(Default::default()))
}

/// Extract a URL string from an endpoint JSON value.
fn extract_url(endpoint: &serde_json::Value) -> String {
    endpoint
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn sub_mode_for_side<'a>(
    engine: &str,
    side_sub_mode: Option<&'a String>,
    legacy_sub_mode: Option<&'a String>,
) -> Option<&'a str> {
    if engine == "gaussdb" {
        return side_sub_mode.or(legacy_sub_mode).map(String::as_str);
    }
    None
}

/// Write an audit log for task management actions.
async fn write_task_audit_log(
    pool: &SqlitePool,
    actor: &str,
    action: &str,
    result: &str,
    target: &str,
    ip: &str,
    details: Option<&str>,
) -> Result<(), ApiError> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let log = OperateLog {
        id: 0,
        actor: actor.to_string(),
        action: action.to_string(),
        result: result.to_string(),
        target: Some(target.to_string()),
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

/// Convert validation errors to an ApiError response.
fn validation_errors_to_response(errors: &[ValidationError]) -> HttpResponse {
    let details: Vec<serde_json::Value> = errors
        .iter()
        .map(|e| {
            serde_json::json!({
                "field": e.field,
                "error": e.error
            })
        })
        .collect();

    let code = if errors.len() == 1 {
        match errors[0].error.as_str() {
            "GAUSSDB_SUB_MODE_REQUIRED" => codes::GAUSSDB_SUB_MODE_REQUIRED,
            s if s.starts_with("UNKNOWN_GAUSSDB_SUB_MODE") => codes::UNKNOWN_GAUSSDB_SUB_MODE,
            "SYNC_MODE_INVALID_FOR_CATEGORY" => codes::SYNC_MODE_INVALID_FOR_CATEGORY,
            "STRUCT_FILTER_REQUIRED" => codes::STRUCT_FILTER_REQUIRED,
            "PATH_OUTSIDE_SANDBOX" => codes::PATH_OUTSIDE_SANDBOX,
            "ENDPOINT_HOST_BLOCKED" => codes::ENDPOINT_HOST_BLOCKED,
            s if s.contains("INVALID_URL_SCHEME") => codes::INVALID_URL_SCHEME,
            s if s.contains("URL_SCHEME_ENGINE_MISMATCH") => codes::URL_SCHEME_ENGINE_MISMATCH,
            _ => codes::TASK_VALIDATION_FAILED,
        }
    } else {
        codes::TASK_VALIDATION_FAILED
    };

    ApiError::with_details(
        code,
        "Validation failed",
        serde_json::json!({ "errors": details }),
    )
    .error_response()
}

/// POST /api/tasks — create a task.
#[post("/tasks")]
pub async fn create_task(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    body: web::Json<CreateTaskRequest>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskCreate) {
        return e.error_response();
    }

    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let source_sub_mode = sub_mode_for_side(
        &body.engine_source,
        body.source_sub_mode.as_ref(),
        body.sub_mode.as_ref(),
    );
    let target_sub_mode = sub_mode_for_side(
        &body.engine_target,
        body.target_sub_mode.as_ref(),
        body.sub_mode.as_ref(),
    );
    let db_type_source = validation::resolve_db_type(&body.engine_source, source_sub_mode);
    let db_type_target = validation::resolve_db_type(&body.engine_target, target_sub_mode);

    // Validate task payload
    let source_url = extract_url(&body.source_endpoint);
    let target_url = extract_url(&body.target_endpoint);

    let errors = validation::validate_task(
        &body.kind,
        &db_type_source,
        &db_type_target,
        &source_url,
        &target_url,
        &body.extractor,
        &body.sinker,
        &body.filter,
        source_sub_mode,
        target_sub_mode,
        true,
    );

    if !errors.is_empty() {
        let _ = write_task_audit_log(
            &pool,
            &user.username,
            "tasks.create",
            "failure",
            "",
            &ip,
            Some(&format!(
                "{{\"reason\":\"validation_failed\",\"count\":{}}}",
                errors.len()
            )),
        )
        .await;
        return validation_errors_to_response(&errors);
    }

    // Path sandboxing
    let path_errors =
        validation::validate_sandboxed_paths(&body.processor, &body.sinker, &body.runtime);
    if !path_errors.is_empty() {
        return validation_errors_to_response(&path_errors);
    }

    // License cap
    if let Err(e) = check_license_cap(&pool).await {
        let _ = write_task_audit_log(
            &pool,
            &user.username,
            "tasks.create",
            "failure",
            "",
            &ip,
            Some(&format!("{{\"reason\":\"{}\"}}", e.code)),
        )
        .await;
        return e.error_response();
    }

    // Resource group
    let rg_id = match &body.resource_group_id {
        Some(rg_id) => match ResourceGroupRepository::find_by_id(&pool, rg_id).await {
            Ok(_) => rg_id.clone(),
            Err(_) => {
                return ApiError::with_details(
                    codes::UNKNOWN_RESOURCE_GROUP,
                    "Unknown resource group",
                    serde_json::json!({ "resource_group_id": rg_id }),
                )
                .error_response();
            }
        },
        None => match ResourceGroupRepository::get_default(&pool).await {
            Ok(rg) => rg.id,
            Err(_) => {
                return ApiError::new(codes::INTERNAL_ERROR, "Default resource group not found")
                    .error_response();
            }
        },
    };

    // Create the task
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let id = uuid::Uuid::new_v4().to_string();
    let task_id = format!(
        "{}_{}_{}_{}",
        body.kind,
        db_type_source,
        db_type_target,
        &id[..8]
    );

    let task = crate::models::Task {
        id: id.clone(),
        task_id: task_id.clone(),
        name: if body.name.is_empty() {
            task_id.clone()
        } else {
            body.name.clone()
        },
        kind: body.kind.clone(),
        db_type_source,
        db_type_target,
        source_endpoint: body.source_endpoint.to_string(),
        target_endpoint: body.target_endpoint.to_string(),
        extractor_config: body.extractor.to_string(),
        sinker_config: body.sinker.to_string(),
        filter_config: body.filter.to_string(),
        router_config: body.router.to_string(),
        parallelizer_config: body.parallelizer.to_string(),
        pipeline_config: body.pipeline.to_string(),
        resumer_config: body.resumer.to_string(),
        processor_config: body.processor.to_string(),
        runtime_config: body.runtime.to_string(),
        metrics_config: body.metrics.to_string(),
        resource_group_id: rg_id,
        owner_user_id: Some(user.user_id.clone()),
        status: "draft".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };

    let saved = match TaskRepository::create(&pool, &task).await {
        Ok(t) => t,
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("task creation failed: {e}"))
                .error_response();
        }
    };

    let _ = write_task_audit_log(
        &pool,
        &user.username,
        "tasks.create",
        "success",
        &id,
        &ip,
        None,
    )
    .await;

    HttpResponse::Created().json(task_to_response(&saved))
}

/// GET /api/tasks — list tasks with optional filters.
#[get("/tasks")]
pub async fn list_tasks(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    query: web::Query<TaskListQuery>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);

    let (tasks, total) = match TaskRepository::list_filtered(
        &pool,
        query.category.as_deref(),
        query.mode.as_deref(),
        query.status.as_deref(),
        query.engine.as_deref(),
        query.q.as_deref(),
        query.resource_group.as_deref(),
        query.sort.as_deref(),
        query.order.as_deref(),
        page,
        page_size,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            let request_id = uuid::Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>();
            tracing::error!(request_id = %request_id, error = %e, "task list failed");
            return ApiError::with_details(
                codes::INTERNAL_ERROR,
                format!("task list failed: {e}"),
                serde_json::json!({ "requestId": request_id }),
            )
            .error_response();
        }
    };

    let items: Vec<TaskResponse> = tasks.iter().map(task_to_response).collect();

    HttpResponse::Ok().json(TaskListResponse {
        items,
        total,
        page,
        page_size,
    })
}

/// GET /api/tasks/:id — get a single task.
#[get("/tasks/{id}")]
pub async fn get_task(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let id = path.into_inner();
    match TaskRepository::find_by_id(&pool, &id).await {
        Ok(task) => HttpResponse::Ok().json(task_to_response(&task)),
        Err(_) => ApiError::with_details(
            codes::TASK_NOT_FOUND,
            "Task not found",
            serde_json::json!({ "id": id }),
        )
        .error_response(),
    }
}

/// PATCH /api/tasks/:id — update a task.
#[patch("/tasks/{id}")]
pub async fn update_task(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    body: web::Json<UpdateTaskRequest>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskUpdate) {
        return e.error_response();
    }

    let id = path.into_inner();
    let mut task = match TaskRepository::find_by_id(&pool, &id).await {
        Ok(t) => t,
        Err(_) => {
            return ApiError::with_details(
                codes::TASK_NOT_FOUND,
                "Task not found",
                serde_json::json!({ "id": id }),
            )
            .error_response();
        }
    };

    // Kind immutability
    if let Some(ref new_kind) = body.kind {
        if new_kind != &task.kind {
            return ApiError::new(
                codes::TASK_KIND_IMMUTABLE,
                "Task kind cannot be changed after creation",
            )
            .error_response();
        }
    }

    // Resource group reassignment check
    if let Some(ref new_rg_id) = body.resource_group_id {
        if new_rg_id != &task.resource_group_id {
            if let Err(e) = check_no_active_run(&pool, &id).await {
                return e.error_response();
            }
            if ResourceGroupRepository::find_by_id(&pool, new_rg_id)
                .await
                .is_err()
            {
                return ApiError::with_details(
                    codes::UNKNOWN_RESOURCE_GROUP,
                    "Unknown resource group",
                    serde_json::json!({ "resource_group_id": new_rg_id }),
                )
                .error_response();
            }
        }
    }

    // Apply updates
    if let Some(ref name) = body.name {
        task.name = name.clone();
    }
    if let Some(ref ep) = body.source_endpoint {
        task.source_endpoint = ep.to_string();
    }
    if let Some(ref ep) = body.target_endpoint {
        task.target_endpoint = ep.to_string();
    }
    if let Some(ref cfg) = body.extractor {
        task.extractor_config = cfg.to_string();
    }
    if let Some(ref cfg) = body.sinker {
        task.sinker_config = cfg.to_string();
    }
    if let Some(ref cfg) = body.filter {
        task.filter_config = cfg.to_string();
    }
    if let Some(ref cfg) = body.router {
        task.router_config = cfg.to_string();
    }
    if let Some(ref cfg) = body.parallelizer {
        task.parallelizer_config = cfg.to_string();
    }
    if let Some(ref cfg) = body.pipeline {
        task.pipeline_config = cfg.to_string();
    }
    if let Some(ref cfg) = body.resumer {
        task.resumer_config = cfg.to_string();
    }
    if let Some(ref cfg) = body.processor {
        task.processor_config = cfg.to_string();
    }
    if let Some(ref cfg) = body.runtime {
        task.runtime_config = cfg.to_string();
    }
    if let Some(ref cfg) = body.metrics {
        task.metrics_config = cfg.to_string();
    }
    if let Some(ref rg_id) = body.resource_group_id {
        task.resource_group_id = rg_id.clone();
    }

    // Validate updated state
    let extractor: serde_json::Value =
        serde_json::from_str(&task.extractor_config).unwrap_or_default();
    let sinker: serde_json::Value = serde_json::from_str(&task.sinker_config).unwrap_or_default();
    let filter: serde_json::Value = serde_json::from_str(&task.filter_config).unwrap_or_default();

    let errors = validation::validate_task(
        &task.kind,
        &task.db_type_source,
        &task.db_type_target,
        &extract_url(&parse_json_or_default(&task.source_endpoint)),
        &extract_url(&parse_json_or_default(&task.target_endpoint)),
        &extractor,
        &sinker,
        &filter,
        None,
        None,
        false,
    );

    if !errors.is_empty() {
        return validation_errors_to_response(&errors);
    }

    // Path sandboxing
    let processor: serde_json::Value =
        serde_json::from_str(&task.processor_config).unwrap_or_default();
    let runtime: serde_json::Value = serde_json::from_str(&task.runtime_config).unwrap_or_default();
    let path_errors = validation::validate_sandboxed_paths(&processor, &sinker, &runtime);
    if !path_errors.is_empty() {
        return validation_errors_to_response(&path_errors);
    }

    // Save
    task.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let saved = match TaskRepository::update(&pool, &task).await {
        Ok(t) => t,
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("task update failed: {e}"))
                .error_response();
        }
    };

    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let _ = write_task_audit_log(
        &pool,
        &user.username,
        "tasks.update",
        "success",
        &id,
        &ip,
        None,
    )
    .await;

    HttpResponse::Ok().json(task_to_response(&saved))
}

/// DELETE /api/tasks/:id — delete a task.
#[delete("/tasks/{id}")]
pub async fn delete_task(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskDelete) {
        return e.error_response();
    }

    let id = path.into_inner();

    if TaskRepository::find_by_id(&pool, &id).await.is_err() {
        return ApiError::with_details(
            codes::TASK_NOT_FOUND,
            "Task not found",
            serde_json::json!({ "id": id }),
        )
        .error_response();
    }

    if let Err(e) = check_no_active_run(&pool, &id).await {
        return e.error_response();
    }

    if let Err(e) = TaskRepository::delete(&pool, &id).await {
        return ApiError::new(codes::INTERNAL_ERROR, format!("task deletion failed: {e}"))
            .error_response();
    }

    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let _ = write_task_audit_log(
        &pool,
        &user.username,
        "tasks.delete",
        "success",
        &id,
        &ip,
        None,
    )
    .await;

    HttpResponse::NoContent().finish()
}

/// Check that no active run exists for the given task.
pub async fn check_no_active_run(pool: &SqlitePool, task_id: &str) -> Result<(), ApiError> {
    let runs = RunRepository::list_by_task(pool, task_id)
        .await
        .map_err(|e| ApiError::new(codes::INTERNAL_ERROR, format!("run list failed: {e}")))?;

    let active_statuses = ["pending", "running", "paused", "stopping"];
    if let Some(active_run) = runs
        .iter()
        .find(|r| active_statuses.contains(&r.status.as_str()))
    {
        return Err(ApiError::with_details(
            codes::TASK_HAS_ACTIVE_RUN,
            "Task has an active run and cannot be modified",
            serde_json::json!({ "run_id": active_run.id, "run_status": active_run.status }),
        ));
    }

    Ok(())
}

/// Query parameters for GET /api/tasks.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TaskListQuery {
    pub category: Option<String>,
    pub mode: Option<String>,
    pub status: Option<String>,
    pub engine: Option<String>,
    pub q: Option<String>,
    pub resource_group: Option<String>,
    pub sort: Option<String>,
    pub order: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

// ── preview_ini, export, import, clone ─────────────────────────────────────

/// GET /api/tasks/:id/preview_ini — render a Task to INI text.
///
/// Returns `Content-Type: text/plain; charset=utf-8` with body byte-identical
/// to what `IniRenderer::render(task)` produces in-process.
#[get("/tasks/{id}/preview_ini")]
pub async fn preview_ini(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let id = path.into_inner();
    match TaskRepository::find_by_id(&pool, &id).await {
        Ok(task) => {
            let ini = crate::ini_renderer::render(&task);
            HttpResponse::Ok()
                .content_type("text/plain; charset=utf-8")
                .body(ini)
        }
        Err(_) => ApiError::with_details(
            codes::TASK_NOT_FOUND,
            "Task not found",
            serde_json::json!({ "id": id }),
        )
        .error_response(),
    }
}

/// Query parameters for GET /api/tasks/:id/export.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExportQuery {
    pub format: Option<String>,
}

/// GET /api/tasks/:id/export?format=json|ini — export a Task.
///
/// - `format=json` (default): returns the full Task DTO as JSON with sensitive fields redacted.
/// - `format=ini`: returns INI text byte-equal to `preview_ini`.
#[get("/tasks/{id}/export")]
pub async fn export_task(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    query: web::Query<ExportQuery>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let id = path.into_inner();
    let format = query.format.as_deref().unwrap_or("json");

    let task = match TaskRepository::find_by_id(&pool, &id).await {
        Ok(t) => t,
        Err(_) => {
            return ApiError::with_details(
                codes::TASK_NOT_FOUND,
                "Task not found",
                serde_json::json!({ "id": id }),
            )
            .error_response();
        }
    };

    match format {
        "json" => {
            let mut resp = task_to_response(&task);
            // Redact sensitive fields in endpoints
            redact_passwords(&mut resp.source_endpoint);
            redact_passwords(&mut resp.target_endpoint);
            HttpResponse::Ok().json(resp)
        }
        "ini" => {
            let ini = crate::ini_renderer::render(&task);
            HttpResponse::Ok()
                .content_type("text/plain; charset=utf-8")
                .body(ini)
        }
        _ => ApiError::with_details(
            codes::UNSUPPORTED_EXPORT_FORMAT,
            "Unsupported export format",
            serde_json::json!({ "format": format, "supported": ["json", "ini"] }),
        )
        .error_response(),
    }
}

/// Redact password fields in a JSON value (mutates in-place).
fn redact_passwords(value: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = value {
        if let Some(serde_json::Value::String(s)) = map.get_mut("password") {
            if !s.is_empty() {
                *s = "<redacted>".to_string();
            }
        }
    }
}

/// Request body for POST /api/tasks/import (single Task import from JSON).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportTaskRequest {
    #[serde(default)]
    pub name: String,
    pub kind: String,
    pub engine_source: String,
    #[serde(default)]
    pub engine_target: String,
    #[serde(default)]
    pub sub_mode: Option<String>,
    #[serde(default)]
    pub source_sub_mode: Option<String>,
    #[serde(default)]
    pub target_sub_mode: Option<String>,
    #[serde(default)]
    pub source_endpoint: serde_json::Value,
    #[serde(default)]
    pub target_endpoint: serde_json::Value,
    #[serde(default)]
    pub extractor: serde_json::Value,
    #[serde(default)]
    pub sinker: serde_json::Value,
    #[serde(default)]
    pub filter: serde_json::Value,
    #[serde(default)]
    pub router: serde_json::Value,
    #[serde(default)]
    pub parallelizer: serde_json::Value,
    #[serde(default)]
    pub pipeline: serde_json::Value,
    #[serde(default)]
    pub resumer: serde_json::Value,
    #[serde(default)]
    pub processor: serde_json::Value,
    #[serde(default)]
    pub runtime: serde_json::Value,
    #[serde(default)]
    pub metrics: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_group_id: Option<String>,
}

/// POST /api/tasks/import — import one or more Tasks from JSON.
///
/// Accepts either a single Task DTO or an array of Task DTOs.
/// Returns 201 for single import; 200 with per-row outcomes for batch.
#[post("/tasks/import")]
pub async fn import_tasks(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    body: web::Json<serde_json::Value>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskCreate) {
        return e.error_response();
    }

    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Determine single vs batch
    if body.is_array() {
        // Batch import
        let items = body.as_array().unwrap();
        let mut successes: Vec<serde_json::Value> = Vec::new();
        let mut failures: Vec<serde_json::Value> = Vec::new();

        for (idx, item) in items.iter().enumerate() {
            match import_single_task(&pool, &user, item, &ip).await {
                Ok(task_resp) => {
                    successes.push(serde_json::json!({
                        "idx": idx,
                        "id": task_resp.id
                    }));
                }
                Err(err_body) => {
                    failures.push(serde_json::json!({
                        "idx": idx,
                        "code": err_body.get("code").and_then(|v| v.as_str()).unwrap_or("INTERNAL_ERROR"),
                        "message": err_body.get("message").and_then(|v| v.as_str()).unwrap_or("import failed")
                    }));
                }
            }
        }

        HttpResponse::Ok().json(serde_json::json!({
            "successes": successes,
            "failures": failures
        }))
    } else {
        // Single import
        match import_single_task(&pool, &user, &body, &ip).await {
            Ok(task_resp) => HttpResponse::Created().json(task_resp),
            Err(err_body) => {
                let status = match err_body.get("code").and_then(|v| v.as_str()) {
                    Some("LICENSE_LIMIT_EXCEEDED") => StatusCode::CONFLICT,
                    Some("TASK_VALIDATION_FAILED")
                    | Some("GAUSSDB_SUB_MODE_REQUIRED")
                    | Some("UNKNOWN_GAUSSDB_SUB_MODE")
                    | Some("SYNC_MODE_INVALID_FOR_CATEGORY")
                    | Some("STRUCT_FILTER_REQUIRED")
                    | Some("PATH_OUTSIDE_SANDBOX")
                    | Some("ENDPOINT_HOST_BLOCKED")
                    | Some("INVALID_URL_SCHEME")
                    | Some("URL_SCHEME_ENGINE_MISMATCH") => StatusCode::UNPROCESSABLE_ENTITY,
                    _ => StatusCode::BAD_REQUEST,
                };
                HttpResponse::build(status).json(err_body)
            }
        }
    }
}

/// Import a single task from a JSON value. Returns the TaskResponse or error envelope.
async fn import_single_task(
    pool: &SqlitePool,
    user: &UserContext,
    body: &serde_json::Value,
    ip: &str,
) -> Result<TaskResponse, serde_json::Value> {
    let import_req: ImportTaskRequest = serde_json::from_value(body.clone()).map_err(|e| {
        serde_json::json!({
            "code": "PARSE_ERROR",
            "message": e.to_string()
        })
    })?;

    let source_sub_mode = sub_mode_for_side(
        &import_req.engine_source,
        import_req.source_sub_mode.as_ref(),
        import_req.sub_mode.as_ref(),
    );
    let target_sub_mode = sub_mode_for_side(
        &import_req.engine_target,
        import_req.target_sub_mode.as_ref(),
        import_req.sub_mode.as_ref(),
    );
    let db_type_source = validation::resolve_db_type(&import_req.engine_source, source_sub_mode);
    let db_type_target = validation::resolve_db_type(&import_req.engine_target, target_sub_mode);

    // Validate
    let source_url = import_req
        .source_endpoint
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let target_url = import_req
        .target_endpoint
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let errors = validation::validate_task(
        &import_req.kind,
        &db_type_source,
        &db_type_target,
        &source_url,
        &target_url,
        &import_req.extractor,
        &import_req.sinker,
        &import_req.filter,
        source_sub_mode,
        target_sub_mode,
        true,
    );

    if !errors.is_empty() {
        let details: Vec<serde_json::Value> = errors
            .iter()
            .map(|e| serde_json::json!({ "field": e.field, "error": e.error }))
            .collect();
        return Err(serde_json::json!({
            "code": "TASK_VALIDATION_FAILED",
            "message": "Validation failed",
            "details": { "errors": details }
        }));
    }

    // Path sandboxing
    let path_errors = validation::validate_sandboxed_paths(
        &import_req.processor,
        &import_req.sinker,
        &import_req.runtime,
    );
    if !path_errors.is_empty() {
        let details: Vec<serde_json::Value> = path_errors
            .iter()
            .map(|e| serde_json::json!({ "field": e.field, "error": e.error }))
            .collect();
        return Err(serde_json::json!({
            "code": "TASK_VALIDATION_FAILED",
            "message": "Validation failed",
            "details": { "errors": details }
        }));
    }

    // License cap
    if let Err(e) = check_license_cap(pool).await {
        return Err(serde_json::json!({
            "code": e.code,
            "message": e.message,
        }));
    }

    // Resource group
    let rg_id = match &import_req.resource_group_id {
        Some(rg_id) => match ResourceGroupRepository::find_by_id(pool, rg_id).await {
            Ok(_) => rg_id.clone(),
            Err(_) => {
                return Err(serde_json::json!({
                    "code": "UNKNOWN_RESOURCE_GROUP",
                    "message": "Unknown resource group"
                }));
            }
        },
        None => match ResourceGroupRepository::get_default(pool).await {
            Ok(rg) => rg.id,
            Err(_) => {
                return Err(serde_json::json!({
                    "code": "INTERNAL_ERROR",
                    "message": "Default resource group not found"
                }));
            }
        },
    };

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let id = uuid::Uuid::new_v4().to_string();
    let task_id = format!(
        "{}_{}_{}_{}",
        import_req.kind,
        db_type_source,
        db_type_target,
        &id[..8]
    );

    let task = crate::models::Task {
        id: id.clone(),
        task_id: task_id.clone(),
        name: if import_req.name.is_empty() {
            task_id.clone()
        } else {
            import_req.name.clone()
        },
        kind: import_req.kind.clone(),
        db_type_source,
        db_type_target,
        source_endpoint: import_req.source_endpoint.to_string(),
        target_endpoint: import_req.target_endpoint.to_string(),
        extractor_config: import_req.extractor.to_string(),
        sinker_config: import_req.sinker.to_string(),
        filter_config: import_req.filter.to_string(),
        router_config: import_req.router.to_string(),
        parallelizer_config: import_req.parallelizer.to_string(),
        pipeline_config: import_req.pipeline.to_string(),
        resumer_config: import_req.resumer.to_string(),
        processor_config: import_req.processor.to_string(),
        runtime_config: import_req.runtime.to_string(),
        metrics_config: import_req.metrics.to_string(),
        resource_group_id: rg_id,
        owner_user_id: Some(user.user_id.clone()),
        status: "draft".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };

    let saved = TaskRepository::create(pool, &task).await.map_err(|e| {
        serde_json::json!({
            "code": "INTERNAL_ERROR",
            "message": format!("task creation failed: {e}")
        })
    })?;

    let _ = write_task_audit_log(
        pool,
        &user.username,
        "tasks.import",
        "success",
        &id,
        ip,
        None,
    )
    .await;

    Ok(task_to_response(&saved))
}

/// POST /api/tasks/:id/clone — clone a Task.
///
/// Creates a new Task with a new `id`, `task_id` (suffixed `_copy_<n>`),
/// `name` (suffixed `(copy)`), and fresh timestamps. Status starts `draft`.
/// Honours the license cap.
#[post("/tasks/{id}/clone")]
pub async fn clone_task(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskCreate) {
        return e.error_response();
    }

    let id = path.into_inner();
    let original = match TaskRepository::find_by_id(&pool, &id).await {
        Ok(t) => t,
        Err(_) => {
            return ApiError::with_details(
                codes::TASK_NOT_FOUND,
                "Task not found",
                serde_json::json!({ "id": id }),
            )
            .error_response();
        }
    };

    // License cap
    if let Err(e) = check_license_cap(&pool).await {
        return e.error_response();
    }

    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let new_id = uuid::Uuid::new_v4().to_string();

    // Find next copy number
    let copy_n = find_next_copy_number(&pool, &original.task_id).await;
    let new_task_id = format!("{}_copy_{copy_n}", original.task_id);
    let new_name = if original.name.is_empty() {
        new_task_id.clone()
    } else {
        format!("{} (copy)", original.name)
    };

    let cloned = crate::models::Task {
        id: new_id.clone(),
        task_id: new_task_id,
        name: new_name,
        kind: original.kind.clone(),
        db_type_source: original.db_type_source.clone(),
        db_type_target: original.db_type_target.clone(),
        source_endpoint: original.source_endpoint.clone(),
        target_endpoint: original.target_endpoint.clone(),
        extractor_config: original.extractor_config.clone(),
        sinker_config: original.sinker_config.clone(),
        filter_config: original.filter_config.clone(),
        router_config: original.router_config.clone(),
        parallelizer_config: original.parallelizer_config.clone(),
        pipeline_config: original.pipeline_config.clone(),
        resumer_config: original.resumer_config.clone(),
        processor_config: original.processor_config.clone(),
        runtime_config: original.runtime_config.clone(),
        metrics_config: original.metrics_config.clone(),
        resource_group_id: original.resource_group_id.clone(),
        owner_user_id: Some(user.user_id.clone()),
        status: "draft".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };

    let saved = match TaskRepository::create(&pool, &cloned).await {
        Ok(t) => t,
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("task clone failed: {e}"))
                .error_response();
        }
    };

    let _ = write_task_audit_log(
        &pool,
        &user.username,
        "tasks.clone",
        "success",
        &new_id,
        &ip,
        None,
    )
    .await;

    HttpResponse::Created().json(task_to_response(&saved))
}

/// Find the next available copy number for a task_id.
async fn find_next_copy_number(pool: &SqlitePool, base_task_id: &str) -> u32 {
    let all_tasks = TaskRepository::list(pool).await.unwrap_or_default();
    let mut max_n: u32 = 0;
    for t in &all_tasks {
        if t.task_id.starts_with(&format!("{base_task_id}_copy_")) {
            let suffix = t.task_id.strip_prefix(&format!("{base_task_id}_copy_"));
            if let Some(n_str) = suffix {
                if let Ok(n) = n_str.parse::<u32>() {
                    max_n = max_n.max(n);
                }
            }
        }
    }
    max_n + 1
}

/// GET /api/tasks/{id}/runs — list runs for a task (paginated).
#[get("/tasks/{id}/runs")]
pub async fn list_task_runs(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    query: web::Query<PageQuery>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let task_id = path.into_inner();
    let page = query.page.unwrap_or(1).max(1);
    let size = query.size.unwrap_or(25).min(100);

    let runs = match RunRepository::list_by_task(&pool, &task_id).await {
        Ok(r) => r,
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("run list failed: {e}"))
                .error_response();
        }
    };

    let total = runs.len();
    let start = ((page - 1) * size) as usize;
    let items: Vec<_> = runs
        .into_iter()
        .skip(start)
        .take(size as usize)
        .map(|r| crate::run_handlers::run_to_response_public(&r))
        .collect();

    HttpResponse::Ok().json(serde_json::json!({
        "items": items,
        "total": total,
    }))
}

/// Query parameters for paginated run list.
#[derive(Debug, serde::Deserialize)]
struct PageQuery {
    page: Option<u32>,
    size: Option<u32>,
}
