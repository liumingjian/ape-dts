//! HTTP handlers for Resource Group CRUD endpoints.
//!
//! - GET    /api/resource_groups          — list resource groups
//! - POST   /api/resource_groups          — create a resource group
//! - GET    /api/resource_groups/:id      — get a single resource group
//! - PATCH  /api/resource_groups/:id      — update a resource group
//! - DELETE /api/resource_groups/:id      — delete a resource group

use actix_web::{delete, get, patch, post, web, HttpResponse, ResponseError};
use sqlx::SqlitePool;

use crate::error::{codes, ApiError};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{
    CreateResourceGroupRequest, OperateLog, ResourceGroupResponse, UpdateResourceGroupRequest,
    UserContext,
};
use crate::repositories::operate_log_repository::OperateLogRepository;
use crate::repositories::resource_group_repository::ResourceGroupRepository;
use crate::repositories::task_repository::TaskRepository;

/// Convert a ResourceGroup model to a ResourceGroupResponse DTO.
fn rg_to_response(rg: &crate::models::ResourceGroup) -> ResourceGroupResponse {
    ResourceGroupResponse {
        id: rg.id.clone(),
        name: rg.name.clone(),
        is_default: rg.is_default,
        created_at: rg.created_at.clone(),
        updated_at: rg.updated_at.clone(),
    }
}

/// Write an audit log for resource group actions.
async fn write_rg_audit_log(
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

/// GET /api/resource_groups — list all resource groups.
#[get("/resource_groups")]
pub async fn list_resource_groups(pool: web::Data<SqlitePool>, user: UserContext) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::ResourceGroupRead) {
        return e.error_response();
    }
    let groups = match ResourceGroupRepository::list(&pool).await {
        Ok(g) => g,
        Err(e) => {
            return ApiError::new(
                codes::INTERNAL_ERROR,
                format!("resource group list failed: {e}"),
            )
            .error_response();
        }
    };

    let items: Vec<ResourceGroupResponse> = groups.iter().map(rg_to_response).collect();
    HttpResponse::Ok().json(items)
}

/// POST /api/resource_groups — create a resource group.
#[post("/resource_groups")]
pub async fn create_resource_group(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    body: web::Json<CreateResourceGroupRequest>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::ResourceGroupCreate) {
        return e.error_response();
    }

    // Check for duplicate name
    if ResourceGroupRepository::find_by_name(&pool, &body.name)
        .await
        .is_ok()
    {
        return ApiError::new(
            codes::RESOURCE_GROUP_NAME_TAKEN,
            "Resource group name already exists",
        )
        .error_response();
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let id = uuid::Uuid::new_v4().to_string();

    let rg = crate::models::ResourceGroup {
        id: id.clone(),
        name: body.name.clone(),
        is_default: false,
        created_at: now.clone(),
        updated_at: now,
    };

    let saved = match ResourceGroupRepository::create(&pool, &rg).await {
        Ok(g) => g,
        Err(e) => {
            return ApiError::new(
                codes::INTERNAL_ERROR,
                format!("resource group creation failed: {e}"),
            )
            .error_response();
        }
    };

    // Audit log
    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let _ = write_rg_audit_log(
        &pool,
        &user.username,
        "resource_groups.create",
        "success",
        &id,
        &ip,
    )
    .await;

    HttpResponse::Created().json(rg_to_response(&saved))
}

/// GET /api/resource_groups/:id — get a single resource group.
#[get("/resource_groups/{id}")]
pub async fn get_resource_group(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::ResourceGroupRead) {
        return e.error_response();
    }
    let id = path.into_inner();
    match ResourceGroupRepository::find_by_id(&pool, &id).await {
        Ok(rg) => HttpResponse::Ok().json(rg_to_response(&rg)),
        Err(_) => ApiError::with_details(
            codes::RESOURCE_GROUP_NOT_FOUND,
            "Resource group not found",
            serde_json::json!({ "id": id }),
        )
        .error_response(),
    }
}

/// PATCH /api/resource_groups/:id — update a resource group.
#[patch("/resource_groups/{id}")]
pub async fn update_resource_group(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    body: web::Json<UpdateResourceGroupRequest>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::ResourceGroupUpdate) {
        return e.error_response();
    }
    let id = path.into_inner();
    let mut rg = match ResourceGroupRepository::find_by_id(&pool, &id).await {
        Ok(g) => g,
        Err(_) => {
            return ApiError::with_details(
                codes::RESOURCE_GROUP_NOT_FOUND,
                "Resource group not found",
                serde_json::json!({ "id": id }),
            )
            .error_response();
        }
    };

    // Default RG protection: cannot rename
    if rg.is_default {
        if let Some(ref new_name) = body.name {
            if new_name != "default" {
                return ApiError::new(
                    codes::DEFAULT_RESOURCE_GROUP_PROTECTED,
                    "Default resource group cannot be renamed",
                )
                .error_response();
            }
        }
    }

    // Apply name update
    if let Some(ref new_name) = body.name {
        // Check duplicate name (if name is actually changing)
        if new_name != &rg.name {
            if ResourceGroupRepository::find_by_name(&pool, new_name)
                .await
                .is_ok()
            {
                return ApiError::new(
                    codes::RESOURCE_GROUP_NAME_TAKEN,
                    "Resource group name already exists",
                )
                .error_response();
            }
            rg.name = new_name.clone();
        }
    }

    rg.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let saved = match ResourceGroupRepository::update(&pool, &rg).await {
        Ok(g) => g,
        Err(e) => {
            return ApiError::new(
                codes::INTERNAL_ERROR,
                format!("resource group update failed: {e}"),
            )
            .error_response();
        }
    };

    // Audit log
    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let _ = write_rg_audit_log(
        &pool,
        &user.username,
        "resource_groups.update",
        "success",
        &id,
        &ip,
    )
    .await;

    HttpResponse::Ok().json(rg_to_response(&saved))
}

/// DELETE /api/resource_groups/:id — delete a resource group.
#[delete("/resource_groups/{id}")]
pub async fn delete_resource_group(
    pool: web::Data<SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::ResourceGroupDelete) {
        return e.error_response();
    }
    let id = path.into_inner();

    let rg = match ResourceGroupRepository::find_by_id(&pool, &id).await {
        Ok(g) => g,
        Err(_) => {
            return ApiError::with_details(
                codes::RESOURCE_GROUP_NOT_FOUND,
                "Resource group not found",
                serde_json::json!({ "id": id }),
            )
            .error_response();
        }
    };

    // Default RG cannot be deleted
    if rg.is_default {
        return ApiError::new(
            codes::DEFAULT_RESOURCE_GROUP_PROTECTED,
            "Default resource group cannot be deleted",
        )
        .error_response();
    }

    // RG with tasks cannot be deleted
    let task_count = match TaskRepository::count_by_resource_group(&pool, &id).await {
        Ok(n) => n,
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("task count failed: {e}"))
                .error_response();
        }
    };

    if task_count > 0 {
        return ApiError::with_details(
            codes::RESOURCE_GROUP_HAS_TASKS,
            "Resource group has tasks and cannot be deleted",
            serde_json::json!({ "task_count": task_count }),
        )
        .error_response();
    }

    // Delete
    if let Err(e) = ResourceGroupRepository::delete(&pool, &id).await {
        return ApiError::new(
            codes::INTERNAL_ERROR,
            format!("resource group deletion failed: {e}"),
        )
        .error_response();
    }

    // Audit log
    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let _ = write_rg_audit_log(
        &pool,
        &user.username,
        "resource_groups.delete",
        "success",
        &id,
        &ip,
    )
    .await;

    HttpResponse::NoContent().finish()
}
