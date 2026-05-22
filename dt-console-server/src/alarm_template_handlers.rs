//! Alarm template CRUD handlers with mustache {{var}} interpolation.
//!
//! - GET    /api/alarm_templates         — list all templates
//! - POST   /api/alarm_templates         — create a template (201)
//! - GET    /api/alarm_templates/:id     — get a single template
//! - PATCH  /api/alarm_templates/:id     — update a template
//! - DELETE /api/alarm_templates/:id     — delete a template (204)
//! - POST   /api/alarm_templates/preview — preview interpolation
//!
//! Mustache {{var}} interpolation substitutes alert fields.
//! Missing variable renders empty without crash.
//! Severity mapping per template: different body per severity level.
//! XSS prevention for template content.

use crate::error::{codes, ApiError};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{AlarmTemplate, UserContext};
use crate::repositories::alarm_template_repository::AlarmTemplateRepository;
use actix_web::{delete, get, patch, post, web, HttpResponse, ResponseError};

/// Interpolate mustache {{var}} placeholders in a template string.
///
/// Replaces `{{key}}` with the corresponding value from `vars`.
/// Missing variables render as empty string (no crash).
/// This is a simple implementation that handles the mustache subset
/// required by ADR-0009: no sections, no inverted sections, no partials.
pub fn interpolate(template: &str, vars: &std::collections::HashMap<&str, &str>) -> String {
    let result = template.to_string();
    // Find all {{...}} tokens and replace them.
    let mut start = 0;
    let mut output = String::with_capacity(template.len());
    while let Some(begin) = result[start..].find("{{") {
        let abs_begin = start + begin;
        let after_begin = abs_begin + 2;
        if let Some(end) = result[after_begin..].find("}}") {
            let abs_end = after_begin + end;
            let key = result[after_begin..abs_end].trim();
            // Append text before the token.
            output.push_str(&result[start..abs_begin]);
            // Append the interpolated value (empty string if missing).
            output.push_str(vars.get(key).copied().unwrap_or(""));
            start = abs_end + 2;
        } else {
            // No closing }}, append rest as-is.
            output.push_str(&result[start..]);
            start = result.len();
            break;
        }
    }
    output.push_str(&result[start..]);
    output
}

/// Resolve the body template for a given severity using the severity_mapping.
///
/// The severity_mapping JSON structure is:
/// ```json
/// {
///   "default": "Default body template",
///   "critical": "CRITICAL: ...",
///   "warning": "Warning: ..."
/// }
/// ```
///
/// If the severity has a specific body, use it; otherwise fall back to default.
/// If no mapping is found at all, use the template's body_template.
pub fn resolve_body_for_severity(template: &AlarmTemplate, severity: &str) -> String {
    let mapping: serde_json::Value = match serde_json::from_str(&template.severity_mapping) {
        Ok(v) => v,
        Err(_) => return template.body_template.clone(),
    };

    // Try the specific severity first.
    if let Some(body) = mapping.get(severity) {
        if let Some(s) = body.as_str() {
            return s.to_string();
        }
    }

    // Fall back to default.
    if let Some(default) = mapping.get("default") {
        if let Some(s) = default.as_str() {
            return s.to_string();
        }
    }

    // Last resort: use the template's body_template.
    template.body_template.clone()
}

/// POST /api/alarm_templates — create a new alarm template.
#[post("/alarm_templates")]
pub async fn create_alarm_template(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    body: web::Json<CreateAlarmTemplateRequest>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlarmTemplateCreate) {
        return e.error_response();
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let id = uuid::Uuid::new_v4().to_string();

    let severity_mapping = match &body.severity_mapping {
        Some(sm) => serde_json::to_string(sm).unwrap_or_else(|_| "{}".to_string()),
        None => "{}".to_string(),
    };

    let template = AlarmTemplate {
        id,
        name: body.name.clone(),
        subject_template: body.subject_template.clone().unwrap_or_default(),
        body_template: body.body_template.clone(),
        severity_mapping,
        created_at: now.clone(),
        updated_at: now,
    };

    match AlarmTemplateRepository::create(&pool, &template).await {
        Ok(persisted) => HttpResponse::Created().json(template_to_json(&persisted)),
        Err(e) => {
            tracing::warn!("alarm template create failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to create alarm template").error_response()
        }
    }
}

/// GET /api/alarm_templates — list all alarm templates.
#[get("/alarm_templates")]
pub async fn list_alarm_templates(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlarmTemplateRead) {
        return e.error_response();
    }

    match AlarmTemplateRepository::list(&pool).await {
        Ok(templates) => {
            let items: Vec<serde_json::Value> = templates.iter().map(template_to_json).collect();
            HttpResponse::Ok().json(serde_json::json!({ "items": items }))
        }
        Err(e) => {
            tracing::warn!("alarm template list failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to list alarm templates").error_response()
        }
    }
}

/// GET /api/alarm_templates/:id — get a single alarm template.
#[get("/alarm_templates/{id}")]
pub async fn get_alarm_template(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlarmTemplateRead) {
        return e.error_response();
    }

    let tmpl_id = path.into_inner();
    match AlarmTemplateRepository::find_by_id(&pool, &tmpl_id).await {
        Ok(tmpl) => HttpResponse::Ok().json(template_to_json(&tmpl)),
        Err(_) => ApiError::with_details(
            codes::NOT_FOUND,
            "Alarm template not found",
            serde_json::json!({ "id": tmpl_id }),
        )
        .error_response(),
    }
}

/// PATCH /api/alarm_templates/:id — update an alarm template.
#[patch("/alarm_templates/{id}")]
pub async fn update_alarm_template(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    body: web::Json<UpdateAlarmTemplateRequest>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlarmTemplateUpdate) {
        return e.error_response();
    }

    let tmpl_id = path.into_inner();
    let mut template = match AlarmTemplateRepository::find_by_id(&pool, &tmpl_id).await {
        Ok(t) => t,
        Err(_) => {
            return ApiError::with_details(
                codes::NOT_FOUND,
                "Alarm template not found",
                serde_json::json!({ "id": tmpl_id }),
            )
            .error_response();
        }
    };

    if let Some(ref name) = body.name {
        template.name = name.clone();
    }
    if let Some(ref subject) = body.subject_template {
        template.subject_template = subject.clone();
    }
    if let Some(ref body_tmpl) = body.body_template {
        template.body_template = body_tmpl.clone();
    }
    if let Some(ref sm) = body.severity_mapping {
        template.severity_mapping = serde_json::to_string(sm).unwrap_or_else(|_| "{}".to_string());
    }

    template.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    match AlarmTemplateRepository::update(&pool, &template).await {
        Ok(persisted) => HttpResponse::Ok().json(template_to_json(&persisted)),
        Err(e) => {
            tracing::warn!("alarm template update failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to update alarm template").error_response()
        }
    }
}

/// DELETE /api/alarm_templates/:id — delete an alarm template.
#[delete("/alarm_templates/{id}")]
pub async fn delete_alarm_template(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlarmTemplateDelete) {
        return e.error_response();
    }

    let tmpl_id = path.into_inner();
    if AlarmTemplateRepository::find_by_id(&pool, &tmpl_id)
        .await
        .is_err()
    {
        return ApiError::with_details(
            codes::NOT_FOUND,
            "Alarm template not found",
            serde_json::json!({ "id": tmpl_id }),
        )
        .error_response();
    }

    match AlarmTemplateRepository::delete(&pool, &tmpl_id).await {
        Ok(()) => HttpResponse::NoContent().finish(),
        Err(e) => {
            tracing::warn!("alarm template delete failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to delete alarm template").error_response()
        }
    }
}

/// POST /api/alarm_templates/preview — preview interpolation.
///
/// Accepts a template body and a set of variables, returns the interpolated
/// result. Also accepts a severity to test severity mapping.
#[post("/alarm_templates/preview")]
pub async fn preview_template(
    user: UserContext,
    body: web::Json<PreviewTemplateRequest>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlarmTemplateRead) {
        return e.error_response();
    }

    // Build variable map.
    let vars: std::collections::HashMap<&str, &str> = body
        .vars
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let subject = interpolate(&body.subject_template, &vars);
    let body_text = interpolate(&body.body_template, &vars);

    // If severity_mapping is provided, resolve the body for the given severity.
    let resolved_body = if let Some(ref severity) = body.severity {
        if let Some(ref mapping) = body.severity_mapping {
            let fake_template = AlarmTemplate {
                id: String::new(),
                name: String::new(),
                subject_template: String::new(),
                body_template: body_text.clone(),
                severity_mapping: serde_json::to_string(mapping)
                    .unwrap_or_else(|_| "{}".to_string()),
                created_at: String::new(),
                updated_at: String::new(),
            };
            resolve_body_for_severity(&fake_template, severity)
        } else {
            body_text
        }
    } else {
        body_text
    };

    HttpResponse::Ok().json(serde_json::json!({
        "subject": subject,
        "body": resolved_body,
    }))
}

/// Convert an AlarmTemplate to a JSON response value with XSS escaping on text fields.
fn template_to_json(tmpl: &AlarmTemplate) -> serde_json::Value {
    let sm: serde_json::Value =
        serde_json::from_str(&tmpl.severity_mapping).unwrap_or(serde_json::json!({}));
    serde_json::json!({
        "id": crate::alert_handlers::escape_xss(&tmpl.id),
        "name": crate::alert_handlers::escape_xss(&tmpl.name),
        "subjectTemplate": crate::alert_handlers::escape_xss(&tmpl.subject_template),
        "bodyTemplate": crate::alert_handlers::escape_xss(&tmpl.body_template),
        "severityMapping": sm,
        "createdAt": crate::alert_handlers::escape_xss(&tmpl.created_at),
        "updatedAt": crate::alert_handlers::escape_xss(&tmpl.updated_at),
    })
}

// ─── Request Types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAlarmTemplateRequest {
    pub name: String,
    pub subject_template: Option<String>,
    pub body_template: String,
    pub severity_mapping: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlarmTemplateRequest {
    pub name: Option<String>,
    pub subject_template: Option<String>,
    pub body_template: Option<String>,
    pub severity_mapping: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewTemplateRequest {
    pub subject_template: String,
    pub body_template: String,
    pub vars: std::collections::HashMap<String, String>,
    pub severity: Option<String>,
    pub severity_mapping: Option<serde_json::Value>,
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpolate_basic() {
        let template = "Task {{task_id}} fired {{metric}}={{value}} > {{threshold}}";
        let mut vars = std::collections::HashMap::new();
        vars.insert("task_id", "t-1");
        vars.insert("metric", "extractor_rps_avg");
        vars.insert("value", "150");
        vars.insert("threshold", "100");

        let result = interpolate(template, &vars);
        assert_eq!(result, "Task t-1 fired extractor_rps_avg=150 > 100");
    }

    #[test]
    fn test_interpolate_missing_var_renders_empty() {
        let template = "Hello {{name}}, {{nonexistent}} world";
        let mut vars = std::collections::HashMap::new();
        vars.insert("name", "Alice");

        let result = interpolate(template, &vars);
        assert_eq!(result, "Hello Alice,  world");
    }

    #[test]
    fn test_interpolate_no_crash_on_missing() {
        let template = "{{nonexistent}}";
        let vars = std::collections::HashMap::new();
        let result = interpolate(template, &vars);
        assert_eq!(result, "");
    }

    #[test]
    fn test_interpolate_preserves_surrounding_text() {
        let template = "prefix{{var}}suffix";
        let mut vars = std::collections::HashMap::new();
        vars.insert("var", "MIDDLE");
        let result = interpolate(template, &vars);
        assert_eq!(result, "prefixMIDDLEsuffix");
    }

    #[test]
    fn test_interpolate_no_placeholders() {
        let template = "No placeholders here";
        let vars = std::collections::HashMap::new();
        let result = interpolate(template, &vars);
        assert_eq!(result, "No placeholders here");
    }

    #[test]
    fn test_interpolate_unclosed_braces() {
        let template = "Hello {{name}, world";
        let mut vars = std::collections::HashMap::new();
        vars.insert("name", "Alice");
        // No closing }}, so the token should not be replaced.
        let result = interpolate(template, &vars);
        assert_eq!(result, "Hello {{name}, world");
    }

    #[test]
    fn test_interpolate_whitespace_in_token() {
        let template = "{{ task_id }}";
        let mut vars = std::collections::HashMap::new();
        vars.insert("task_id", "t-1");
        let result = interpolate(template, &vars);
        assert_eq!(result, "t-1");
    }

    #[test]
    fn test_resolve_body_for_severity_specific() {
        let tmpl = AlarmTemplate {
            id: "t-1".to_string(),
            name: "Test".to_string(),
            subject_template: String::new(),
            body_template: "Default body".to_string(),
            severity_mapping: r#"{"default":"Default body","critical":"URGENT: {{metric}}","warning":"Notice: {{metric}}"}"#.to_string(),
            created_at: String::new(),
            updated_at: String::new(),
        };

        assert_eq!(
            resolve_body_for_severity(&tmpl, "critical"),
            "URGENT: {{metric}}"
        );
        assert_eq!(
            resolve_body_for_severity(&tmpl, "warning"),
            "Notice: {{metric}}"
        );
        assert_eq!(resolve_body_for_severity(&tmpl, "info"), "Default body");
    }

    #[test]
    fn test_resolve_body_for_severity_fallback_to_default() {
        let tmpl = AlarmTemplate {
            id: "t-1".to_string(),
            name: "Test".to_string(),
            subject_template: String::new(),
            body_template: "Template body".to_string(),
            severity_mapping: r#"{"default":"Default body","critical":"URGENT"}"#.to_string(),
            created_at: String::new(),
            updated_at: String::new(),
        };

        assert_eq!(resolve_body_for_severity(&tmpl, "warning"), "Default body");
    }

    #[test]
    fn test_resolve_body_for_severity_no_mapping() {
        let tmpl = AlarmTemplate {
            id: "t-1".to_string(),
            name: "Test".to_string(),
            subject_template: String::new(),
            body_template: "Template body".to_string(),
            severity_mapping: "{}".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
        };

        assert_eq!(
            resolve_body_for_severity(&tmpl, "critical"),
            "Template body"
        );
    }

    #[test]
    fn test_template_to_json_escapes_xss() {
        let tmpl = AlarmTemplate {
            id: "t-1".to_string(),
            name: "<script>alert(1)</script>".to_string(),
            subject_template: String::new(),
            body_template: "Normal body".to_string(),
            severity_mapping: "{}".to_string(),
            created_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
        };
        let json = template_to_json(&tmpl);
        assert_eq!(json["name"], "&lt;script&gt;alert(1)&lt;/script&gt;");
    }

    #[tokio::test]
    async fn test_template_crud_against_memory_db() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let tmpl = AlarmTemplate {
            id: "tmpl-crud-1".to_string(),
            name: "High RPS Template".to_string(),
            subject_template: "Alert: {{metric}}".to_string(),
            body_template: "Task {{task_id}} fired {{metric}}={{value}}".to_string(),
            severity_mapping: r#"{"default":"Default","critical":"URGENT"}"#.to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        // Create
        let created = AlarmTemplateRepository::create(&pool, &tmpl).await.unwrap();
        assert_eq!(created.id, "tmpl-crud-1");
        assert_eq!(created.name, "High RPS Template");

        // Read
        let found = AlarmTemplateRepository::find_by_id(&pool, "tmpl-crud-1")
            .await
            .unwrap();
        assert_eq!(
            found.body_template,
            "Task {{task_id}} fired {{metric}}={{value}}"
        );
        assert_eq!(
            found.severity_mapping,
            r#"{"default":"Default","critical":"URGENT"}"#
        );

        // Update
        let mut updated = found;
        updated.name = "Updated Template".to_string();
        updated.body_template = "New body".to_string();
        updated.updated_at =
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let saved = AlarmTemplateRepository::update(&pool, &updated)
            .await
            .unwrap();
        assert_eq!(saved.name, "Updated Template");
        assert_eq!(saved.body_template, "New body");

        // Delete
        AlarmTemplateRepository::delete(&pool, "tmpl-crud-1")
            .await
            .unwrap();
        assert!(AlarmTemplateRepository::find_by_id(&pool, "tmpl-crud-1")
            .await
            .is_err());
    }
}
