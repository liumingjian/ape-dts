//! Alarm channel CRUD handlers.
//!
//! - GET    /api/alarm_channels         — list all channels
//! - POST   /api/alarm_channels         — create a channel (201)
//! - GET    /api/alarm_channels/:id     — get a single channel
//! - PATCH  /api/alarm_channels/:id     — update a channel
//! - DELETE /api/alarm_channels/:id     — delete a channel (204)
//!
//! Supported channel kinds: kafka, snmp.
//! XSS prevention on user-supplied text fields.

use crate::error::{codes, ApiError};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{AlarmChannel, UserContext};
use crate::repositories::alarm_channel_repository::AlarmChannelRepository;
use actix_web::{delete, get, patch, post, web, HttpResponse, ResponseError};

/// Valid alarm channel kinds.
const VALID_KINDS: &[&str] = &["kafka", "snmp"];

/// POST /api/alarm_channels — create a new alarm channel.
#[post("/alarm_channels")]
pub async fn create_alarm_channel(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    body: web::Json<CreateAlarmChannelRequest>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlarmChannelCreate) {
        return e.error_response();
    }

    let kind_lower = body.kind.to_lowercase();
    if !VALID_KINDS.contains(&kind_lower.as_str()) {
        return ApiError::with_details(
            codes::VALIDATION_FAILED,
            "Invalid alarm channel kind",
            serde_json::json!({
                "field": "kind",
                "valid_kinds": VALID_KINDS,
                "got": body.kind,
            }),
        )
        .error_response();
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let id = uuid::Uuid::new_v4().to_string();
    let config = serde_json::to_string(&body.config).unwrap_or_else(|_| "{}".to_string());

    let channel = AlarmChannel {
        id,
        name: body.name.clone(),
        kind: kind_lower,
        config,
        enabled: body.enabled.unwrap_or(true),
        resource_group_id: body.resource_group_id.clone(),
        created_at: now.clone(),
        updated_at: now,
    };

    match AlarmChannelRepository::create(&pool, &channel).await {
        Ok(persisted) => HttpResponse::Created().json(channel_to_json(&persisted)),
        Err(e) => {
            tracing::warn!("alarm channel create failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to create alarm channel").error_response()
        }
    }
}

/// GET /api/alarm_channels — list all alarm channels.
#[get("/alarm_channels")]
pub async fn list_alarm_channels(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlarmChannelRead) {
        return e.error_response();
    }

    match AlarmChannelRepository::list(&pool).await {
        Ok(channels) => {
            let items: Vec<serde_json::Value> = channels.iter().map(channel_to_json).collect();
            HttpResponse::Ok().json(serde_json::json!({ "items": items }))
        }
        Err(e) => {
            tracing::warn!("alarm channel list failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to list alarm channels").error_response()
        }
    }
}

/// GET /api/alarm_channels/:id — get a single alarm channel.
#[get("/alarm_channels/{id}")]
pub async fn get_alarm_channel(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlarmChannelRead) {
        return e.error_response();
    }

    let ch_id = path.into_inner();
    match AlarmChannelRepository::find_by_id(&pool, &ch_id).await {
        Ok(ch) => HttpResponse::Ok().json(channel_to_json(&ch)),
        Err(_) => ApiError::with_details(
            codes::NOT_FOUND,
            "Alarm channel not found",
            serde_json::json!({ "id": ch_id }),
        )
        .error_response(),
    }
}

/// PATCH /api/alarm_channels/:id — update an alarm channel.
#[patch("/alarm_channels/{id}")]
pub async fn update_alarm_channel(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    body: web::Json<UpdateAlarmChannelRequest>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlarmChannelUpdate) {
        return e.error_response();
    }

    let ch_id = path.into_inner();
    let mut channel = match AlarmChannelRepository::find_by_id(&pool, &ch_id).await {
        Ok(ch) => ch,
        Err(_) => {
            return ApiError::with_details(
                codes::NOT_FOUND,
                "Alarm channel not found",
                serde_json::json!({ "id": ch_id }),
            )
            .error_response();
        }
    };

    if let Some(ref name) = body.name {
        channel.name = name.clone();
    }
    if let Some(ref kind) = body.kind {
        let kind_lower = kind.to_lowercase();
        if !VALID_KINDS.contains(&kind_lower.as_str()) {
            return ApiError::with_details(
                codes::VALIDATION_FAILED,
                "Invalid alarm channel kind",
                serde_json::json!({
                    "field": "kind",
                    "valid_kinds": VALID_KINDS,
                    "got": kind,
                }),
            )
            .error_response();
        }
        channel.kind = kind_lower;
    }
    if let Some(ref config) = body.config {
        // The client only ever saw a redacted config, so put back any secret it
        // echoed as the placeholder rather than persisting the placeholder.
        let mut merged = config.clone();
        let stored: serde_json::Value =
            serde_json::from_str(&channel.config).unwrap_or(serde_json::json!({}));
        crate::redaction::restore_secrets(&mut merged, &stored);
        channel.config = serde_json::to_string(&merged).unwrap_or_else(|_| "{}".to_string());
    }
    if let Some(enabled) = body.enabled {
        channel.enabled = enabled;
    }
    if body.resource_group_id.is_some() {
        channel.resource_group_id = body.resource_group_id.clone();
    }

    channel.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    match AlarmChannelRepository::update(&pool, &channel).await {
        Ok(persisted) => HttpResponse::Ok().json(channel_to_json(&persisted)),
        Err(e) => {
            tracing::warn!("alarm channel update failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to update alarm channel").error_response()
        }
    }
}

/// DELETE /api/alarm_channels/:id — delete an alarm channel.
#[delete("/alarm_channels/{id}")]
pub async fn delete_alarm_channel(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlarmChannelDelete) {
        return e.error_response();
    }

    let ch_id = path.into_inner();
    if AlarmChannelRepository::find_by_id(&pool, &ch_id)
        .await
        .is_err()
    {
        return ApiError::with_details(
            codes::NOT_FOUND,
            "Alarm channel not found",
            serde_json::json!({ "id": ch_id }),
        )
        .error_response();
    }

    match AlarmChannelRepository::delete(&pool, &ch_id).await {
        Ok(()) => HttpResponse::NoContent().finish(),
        Err(e) => {
            tracing::warn!("alarm channel delete failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to delete alarm channel").error_response()
        }
    }
}

/// Convert an AlarmChannel to a JSON response value with XSS escaping.
///
/// Channel configs carry webhook URLs, bot tokens, SMTP passwords and SNMP
/// communities, so the config is redacted on the way out; writes restore the
/// stored value when the client echoes the placeholder back.
fn channel_to_json(ch: &AlarmChannel) -> serde_json::Value {
    let mut config: serde_json::Value =
        serde_json::from_str(&ch.config).unwrap_or(serde_json::json!({}));
    crate::redaction::redact_secrets(&mut config);
    serde_json::json!({
        "id": crate::alert_handlers::escape_xss(&ch.id),
        "name": crate::alert_handlers::escape_xss(&ch.name),
        "kind": crate::alert_handlers::escape_xss(&ch.kind),
        "config": config,
        "enabled": ch.enabled,
        "resourceGroupId": ch.resource_group_id,
        "createdAt": crate::alert_handlers::escape_xss(&ch.created_at),
        "updatedAt": crate::alert_handlers::escape_xss(&ch.updated_at),
    })
}

// ─── Request Types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAlarmChannelRequest {
    pub name: String,
    pub kind: String,
    pub config: serde_json::Value,
    pub enabled: Option<bool>,
    pub resource_group_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlarmChannelRequest {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub config: Option<serde_json::Value>,
    pub enabled: Option<bool>,
    pub resource_group_id: Option<String>,
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_kinds() {
        assert!(VALID_KINDS.contains(&"kafka"));
        assert!(VALID_KINDS.contains(&"snmp"));
    }

    #[test]
    fn test_channel_to_json_escapes_xss() {
        let ch = AlarmChannel {
            id: "ch-1".to_string(),
            name: "<script>alert(1)</script>".to_string(),
            kind: "kafka".to_string(),
            config: r#"{"brokers":"localhost:9092"}"#.to_string(),
            enabled: true,
            resource_group_id: None,
            created_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
        };
        let json = channel_to_json(&ch);
        assert_eq!(json["name"], "&lt;script&gt;alert(1)&lt;/script&gt;");
        assert_eq!(json["kind"], "kafka");
        assert_eq!(json["enabled"], true);
    }

    #[tokio::test]
    async fn test_channel_crud_against_memory_db() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let ch = AlarmChannel {
            id: "ch-crud-1".to_string(),
            name: "Kafka Channel".to_string(),
            kind: "kafka".to_string(),
            config: r#"{"brokers":"localhost:9092","topic":"alerts"}"#.to_string(),
            enabled: true,
            resource_group_id: None,
            created_at: now.clone(),
            updated_at: now,
        };

        // Create
        let created = AlarmChannelRepository::create(&pool, &ch).await.unwrap();
        assert_eq!(created.id, "ch-crud-1");
        assert_eq!(created.kind, "kafka");

        // Read
        let found = AlarmChannelRepository::find_by_id(&pool, "ch-crud-1")
            .await
            .unwrap();
        assert_eq!(found.name, "Kafka Channel");
        assert!(found.enabled);

        // Update
        let mut updated = found;
        updated.name = "Updated Channel".to_string();
        updated.enabled = false;
        updated.updated_at =
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let saved = AlarmChannelRepository::update(&pool, &updated)
            .await
            .unwrap();
        assert_eq!(saved.name, "Updated Channel");
        assert!(!saved.enabled);

        // Delete
        AlarmChannelRepository::delete(&pool, "ch-crud-1")
            .await
            .unwrap();
        assert!(AlarmChannelRepository::find_by_id(&pool, "ch-crud-1")
            .await
            .is_err());
    }
}
