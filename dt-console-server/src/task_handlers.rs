//! HTTP handlers for Task CRUD endpoints.
//!
//! - POST   /api/tasks          — create a task
//! - GET    /api/tasks          — list tasks (with filters)
//! - GET    /api/tasks/:id      — get a single task
//! - PATCH  /api/tasks/:id      — update a task
//! - DELETE /api/tasks/:id      — delete a task

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
            "gaussdb_sub_mode_required" => codes::GAUSSDB_SUB_MODE_REQUIRED,
            s if s.starts_with("unknown_gaussdb_sub_mode") => codes::UNKNOWN_GAUSSDB_SUB_MODE,
            "sync_mode_invalid_for_category" => codes::SYNC_MODE_INVALID_FOR_CATEGORY,
            "struct_filter_required" => codes::STRUCT_FILTER_REQUIRED,
            "path_outside_sandbox" => codes::PATH_OUTSIDE_SANDBOX,
            "endpoint_host_blocked" => codes::ENDPOINT_HOST_BLOCKED,
            s if s.contains("invalid_url_scheme") => codes::INVALID_URL_SCHEME,
            s if s.contains("url_scheme_engine_mismatch") => codes::URL_SCHEME_ENGINE_MISMATCH,
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

    // Resolve db_type from engine + sub_mode
    let db_type_source = validation::resolve_db_type(&body.engine_source, body.sub_mode.as_deref());
    let db_type_target = validation::resolve_db_type(&body.engine_target, body.sub_mode.as_deref());

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
        body.sub_mode.as_deref(),
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
        query.status.as_deref(),
        query.engine.as_deref(),
        query.q.as_deref(),
        query.resource_group.as_deref(),
        page,
        page_size,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("task list failed: {e}"))
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
#[serde(rename_all = "camelCase")]
pub struct TaskListQuery {
    pub category: Option<String>,
    pub status: Option<String>,
    pub engine: Option<String>,
    pub q: Option<String>,
    pub resource_group: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}
