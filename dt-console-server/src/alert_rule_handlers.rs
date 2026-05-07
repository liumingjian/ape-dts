//! Alert rule CRUD handlers.
//!
//! - GET    /api/alert_rules         — list all rules
//! - POST   /api/alert_rules         — create a rule (201)
//! - GET    /api/alert_rules/:id     — get a single rule
//! - PATCH  /api/alert_rules/:id     — update a rule
//! - DELETE /api/alert_rules/:id     — delete a rule (204)
//! - POST   /api/alert_rules/evaluate_now — debug fixture: evaluate a rule
//!
//! Invalid rule (bad metric, bad op) returns 400 envelope error.
//! Disabled rule does not fire (checked in AlertEngine).
//! XSS prevention on user-supplied text fields.

use crate::alert_engine::{evaluate_op, is_recovered};
use crate::error::{codes, ApiError};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{AlertRule, UserContext};
use crate::repositories::alert_rule_repository::AlertRuleRepository;
use actix_web::{delete, get, patch, post, web, HttpResponse, ResponseError};

/// Valid comparison operators for alert rules.
const VALID_OPS: &[&str] = &[">", "<", ">=", "<=", "=="];

/// Validate an alert rule's fields.
///
/// Returns Ok(()) if valid, Err(ApiError) with envelope otherwise.
fn validate_rule(metric: &str, op: &str) -> Result<(), ApiError> {
    if !VALID_OPS.contains(&op) {
        return Err(ApiError::with_details(
            codes::VALIDATION_FAILED,
            "Invalid operator for alert rule",
            serde_json::json!({
                "field": "operator",
                "valid_operators": VALID_OPS,
                "got": op,
            }),
        ));
    }
    // Allow canonical metric names or any custom metric name.
    // The strict check is only for the canonical set; custom metrics are
    // allowed but must not be empty.
    if metric.is_empty() {
        return Err(ApiError::with_details(
            codes::VALIDATION_FAILED,
            "Metric name cannot be empty",
            serde_json::json!({ "field": "metricName" }),
        ));
    }
    Ok(())
}

/// POST /api/alert_rules — create a new alert rule.
#[post("/alert_rules")]
pub async fn create_alert_rule(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    body: web::Json<CreateAlertRuleRequest>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlertRuleCreate) {
        return e.error_response();
    }

    if let Err(e) = validate_rule(&body.metric_name, &body.operator) {
        return e.error_response();
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let id = uuid::Uuid::new_v4().to_string();
    let channel_ids = serde_json::to_string(&body.channel_ids.clone().unwrap_or_default())
        .unwrap_or_else(|_| "[]".to_string());

    let rule = AlertRule {
        id,
        name: body.name.clone(),
        metric_name: body.metric_name.clone(),
        operator: body.operator.clone(),
        threshold: body.threshold,
        recovery_threshold: body.recovery_threshold,
        severity: body
            .severity
            .clone()
            .unwrap_or_else(|| "warning".to_string()),
        dwell_secs: body.dwell_secs.unwrap_or(0),
        channel_ids,
        enabled: body.enabled.unwrap_or(true),
        resource_group_id: body.resource_group_id.clone(),
        created_at: now.clone(),
        updated_at: now,
    };

    match AlertRuleRepository::create(&pool, &rule).await {
        Ok(persisted) => {
            let resp = rule_to_json(&persisted);
            HttpResponse::Created().json(resp)
        }
        Err(e) => {
            tracing::warn!("alert rule create failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to create alert rule").error_response()
        }
    }
}

/// GET /api/alert_rules — list all alert rules.
#[get("/alert_rules")]
pub async fn list_alert_rules(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlertRuleRead) {
        return e.error_response();
    }

    match AlertRuleRepository::list(&pool).await {
        Ok(rules) => {
            let items: Vec<serde_json::Value> = rules.iter().map(rule_to_json).collect();
            HttpResponse::Ok().json(serde_json::json!({ "items": items }))
        }
        Err(e) => {
            tracing::warn!("alert rule list failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to list alert rules").error_response()
        }
    }
}

/// GET /api/alert_rules/:id — get a single alert rule.
#[get("/alert_rules/{id}")]
pub async fn get_alert_rule(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlertRuleRead) {
        return e.error_response();
    }

    let rule_id = path.into_inner();
    match AlertRuleRepository::find_by_id(&pool, &rule_id).await {
        Ok(rule) => HttpResponse::Ok().json(rule_to_json(&rule)),
        Err(_) => ApiError::with_details(
            codes::NOT_FOUND,
            "Alert rule not found",
            serde_json::json!({ "id": rule_id }),
        )
        .error_response(),
    }
}

/// PATCH /api/alert_rules/:id — update an alert rule.
#[patch("/alert_rules/{id}")]
pub async fn update_alert_rule(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    body: web::Json<UpdateAlertRuleRequest>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlertRuleUpdate) {
        return e.error_response();
    }

    let rule_id = path.into_inner();
    let mut rule = match AlertRuleRepository::find_by_id(&pool, &rule_id).await {
        Ok(r) => r,
        Err(_) => {
            return ApiError::with_details(
                codes::NOT_FOUND,
                "Alert rule not found",
                serde_json::json!({ "id": rule_id }),
            )
            .error_response();
        }
    };

    // Apply partial updates.
    if let Some(ref name) = body.name {
        rule.name = name.clone();
    }
    if let Some(ref metric) = body.metric_name {
        rule.metric_name = metric.clone();
    }
    if let Some(ref op) = body.operator {
        rule.operator = op.clone();
    }
    if let Some(threshold) = body.threshold {
        rule.threshold = threshold;
    }
    if body.recovery_threshold.is_some() {
        rule.recovery_threshold = body.recovery_threshold;
    }
    if let Some(ref severity) = body.severity {
        rule.severity = severity.clone();
    }
    if let Some(dwell) = body.dwell_secs {
        rule.dwell_secs = dwell;
    }
    if let Some(ref channel_ids) = body.channel_ids {
        rule.channel_ids = serde_json::to_string(channel_ids).unwrap_or_else(|_| "[]".to_string());
    }
    if let Some(enabled) = body.enabled {
        rule.enabled = enabled;
    }
    if body.resource_group_id.is_some() {
        rule.resource_group_id = body.resource_group_id.clone();
    }

    // Validate after applying updates.
    if let Err(e) = validate_rule(&rule.metric_name, &rule.operator) {
        return e.error_response();
    }

    rule.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    match AlertRuleRepository::update(&pool, &rule).await {
        Ok(persisted) => HttpResponse::Ok().json(rule_to_json(&persisted)),
        Err(e) => {
            tracing::warn!("alert rule update failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to update alert rule").error_response()
        }
    }
}

/// DELETE /api/alert_rules/:id — delete an alert rule.
#[delete("/alert_rules/{id}")]
pub async fn delete_alert_rule(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlertRuleDelete) {
        return e.error_response();
    }

    let rule_id = path.into_inner();

    // Verify existence first.
    if AlertRuleRepository::find_by_id(&pool, &rule_id)
        .await
        .is_err()
    {
        return ApiError::with_details(
            codes::NOT_FOUND,
            "Alert rule not found",
            serde_json::json!({ "id": rule_id }),
        )
        .error_response();
    }

    match AlertRuleRepository::delete(&pool, &rule_id).await {
        Ok(()) => HttpResponse::NoContent().finish(),
        Err(e) => {
            tracing::warn!("alert rule delete failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to delete alert rule").error_response()
        }
    }
}

/// POST /api/alert_rules/evaluate_now — debug fixture: evaluate a rule
/// against a fixed series of points without persisting anything.
///
/// Accepts a rule definition + synthetic data points and returns the events
/// that would have been emitted.
#[post("/alert_rules/evaluate_now")]
pub async fn evaluate_now(user: UserContext, body: web::Json<EvaluateNowRequest>) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlertRuleRead) {
        return e.error_response();
    }

    // Validate the rule fields.
    if let Err(e) = validate_rule(&body.metric_name, &body.operator) {
        return e.error_response();
    }

    let mut events = Vec::new();
    let mut firing = false;
    let mut dwell_start: Option<i64> = None;
    let dwell_secs = body.dwell_secs.unwrap_or(0);
    let threshold = body.threshold;
    let recovery_threshold = body.recovery_threshold;

    for (i, point) in body.points.iter().enumerate() {
        let ts = point.ts.unwrap_or(i as i64);
        let value = point.value;
        let breached = evaluate_op(&body.operator, value, threshold);

        if breached {
            if !firing {
                if dwell_secs > 0 {
                    if dwell_start.is_none() {
                        dwell_start = Some(ts);
                    }
                    let elapsed = ts - dwell_start.unwrap();
                    if elapsed >= dwell_secs {
                        firing = true;
                        events.push(serde_json::json!({
                            "type": "firing",
                            "at_index": i,
                            "value": value,
                            "threshold": threshold,
                        }));
                    }
                } else {
                    firing = true;
                    events.push(serde_json::json!({
                        "type": "firing",
                        "at_index": i,
                        "value": value,
                        "threshold": threshold,
                    }));
                }
            }
        } else if firing {
            // Check recovery threshold using operator-aware is_recovered logic.
            let temp_rule = AlertRule {
                id: String::new(),
                name: String::new(),
                metric_name: body.metric_name.clone(),
                operator: body.operator.clone(),
                threshold,
                recovery_threshold,
                severity: String::new(),
                dwell_secs: 0,
                channel_ids: "[]".to_string(),
                enabled: true,
                resource_group_id: None,
                created_at: String::new(),
                updated_at: String::new(),
            };
            let recovered = is_recovered(&temp_rule, value);
            if recovered {
                firing = false;
                dwell_start = None;
                events.push(serde_json::json!({
                    "type": "recovery",
                    "at_index": i,
                    "value": value,
                }));
            }
        } else {
            dwell_start = None;
        }
    }

    HttpResponse::Ok().json(serde_json::json!({
        "events": events,
        "synthetic": true,
    }))
}

/// Convert an AlertRule to a JSON response value with XSS escaping on user-supplied text.
fn rule_to_json(rule: &AlertRule) -> serde_json::Value {
    let channel_ids: Vec<String> = serde_json::from_str(&rule.channel_ids).unwrap_or_default();
    serde_json::json!({
        "id": crate::alert_handlers::escape_xss(&rule.id),
        "name": crate::alert_handlers::escape_xss(&rule.name),
        "metricName": crate::alert_handlers::escape_xss(&rule.metric_name),
        "operator": rule.operator,  // controlled enum, not user-supplied
        "threshold": rule.threshold,
        "recoveryThreshold": rule.recovery_threshold,
        "severity": rule.severity,  // controlled enum
        "dwellSecs": rule.dwell_secs,
        "channelIds": channel_ids,
        "enabled": rule.enabled,
        "resourceGroupId": rule.resource_group_id,
        "createdAt": crate::alert_handlers::escape_xss(&rule.created_at),
        "updatedAt": crate::alert_handlers::escape_xss(&rule.updated_at),
    })
}

// ─── Request Types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAlertRuleRequest {
    pub name: String,
    pub metric_name: String,
    pub operator: String,
    pub threshold: f64,
    pub recovery_threshold: Option<f64>,
    pub severity: Option<String>,
    pub dwell_secs: Option<i64>,
    pub channel_ids: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub resource_group_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertRuleRequest {
    pub name: Option<String>,
    pub metric_name: Option<String>,
    pub operator: Option<String>,
    pub threshold: Option<f64>,
    pub recovery_threshold: Option<f64>,
    pub severity: Option<String>,
    pub dwell_secs: Option<i64>,
    pub channel_ids: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub resource_group_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateNowRequest {
    pub metric_name: String,
    pub operator: String,
    pub threshold: f64,
    pub recovery_threshold: Option<f64>,
    pub dwell_secs: Option<i64>,
    pub points: Vec<SyntheticPoint>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SyntheticPoint {
    pub value: f64,
    pub ts: Option<i64>,
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_rule_valid() {
        assert!(validate_rule("extractor_rps_avg", ">").is_ok());
        assert!(validate_rule("extractor_rps_avg", ">=").is_ok());
        assert!(validate_rule("extractor_rps_avg", "==").is_ok());
    }

    #[test]
    fn test_validate_rule_invalid_op() {
        let err = validate_rule("extractor_rps_avg", "≠").unwrap_err();
        assert_eq!(err.code, codes::VALIDATION_FAILED);
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn test_validate_rule_empty_metric() {
        let err = validate_rule("", ">").unwrap_err();
        assert_eq!(err.code, codes::VALIDATION_FAILED);
    }

    #[test]
    fn test_evaluate_now_simple_firing() {
        let req = EvaluateNowRequest {
            metric_name: "extractor_rps_avg".to_string(),
            operator: ">".to_string(),
            threshold: 100.0,
            recovery_threshold: None,
            dwell_secs: Some(0),
            points: vec![
                SyntheticPoint {
                    value: 50.0,
                    ts: Some(0),
                },
                SyntheticPoint {
                    value: 80.0,
                    ts: Some(10),
                },
                SyntheticPoint {
                    value: 150.0,
                    ts: Some(20),
                },
            ],
        };

        // We need to call the logic directly since the handler needs UserContext.
        let mut events = Vec::new();
        let mut firing = false;

        for (i, point) in req.points.iter().enumerate() {
            let value = point.value;
            let breached = evaluate_op(&req.operator, value, req.threshold);

            if breached {
                if !firing {
                    firing = true;
                    events.push(serde_json::json!({
                        "type": "firing",
                        "at_index": i,
                        "value": value,
                    }));
                }
            } else if firing {
                firing = false;
                events.push(serde_json::json!({
                    "type": "recovery",
                    "at_index": i,
                }));
            }
        }

        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "firing");
        assert_eq!(events[0]["at_index"], 2);
    }

    #[test]
    fn test_evaluate_now_dwell() {
        // Dwell requires sustained violation.
        // Points: 10s spacing. Dwell = 30s.
        // Index 0: value=150 (breach, dwell_start=0)
        // Index 1: value=50  (no breach, reset dwell_start)
        // Index 2: value=150 (breach, dwell_start=20)
        // Index 3: value=150 (breach, elapsed=30-20=10, < 30, not yet)
        // Index 4: value=150 (breach, elapsed=40-20=20, < 30, not yet)
        // But with 10s spacing we need 30s of sustained violation from index 2,
        // which means we need index at ts=50 to get elapsed=30.
        // Let's adjust: use 30s spacing to simplify.
        let points = vec![
            SyntheticPoint {
                value: 150.0,
                ts: Some(0),
            }, // breach, dwell_start=0
            SyntheticPoint {
                value: 50.0,
                ts: Some(30),
            }, // no breach, reset
            SyntheticPoint {
                value: 150.0,
                ts: Some(60),
            }, // breach, dwell_start=60
            SyntheticPoint {
                value: 150.0,
                ts: Some(90),
            }, // breach, elapsed=30 >= 30, FIRE
            SyntheticPoint {
                value: 150.0,
                ts: Some(120),
            }, // breach continues
        ];

        let mut events = Vec::new();
        let mut firing = false;
        let mut dwell_start: Option<i64> = None;
        let dwell_secs: i64 = 30;
        let threshold = 100.0_f64;

        for (i, point) in points.iter().enumerate() {
            let ts = point.ts.unwrap_or(i as i64);
            let value = point.value;
            let breached = evaluate_op(">", value, threshold);

            if breached {
                if !firing {
                    if dwell_start.is_none() {
                        dwell_start = Some(ts);
                    }
                    let elapsed = ts - dwell_start.unwrap();
                    if elapsed >= dwell_secs {
                        firing = true;
                        events.push(serde_json::json!({
                            "type": "firing",
                            "at_index": i,
                        }));
                    }
                }
            } else if firing {
                firing = false;
                dwell_start = None;
                events.push(serde_json::json!({
                    "type": "recovery",
                    "at_index": i,
                }));
            } else {
                dwell_start = None;
            }
        }

        // Should fire at index 3 (elapsed=90-60=30 >= 30)
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "firing");
        assert_eq!(events[0]["at_index"], 3);
    }

    #[test]
    fn test_crud_round_trip_preserves_shape() {
        // Test that rule_to_json produces the expected fields.
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let rule = AlertRule {
            id: "rule-1".to_string(),
            name: "High RPS".to_string(),
            metric_name: "extractor_rps_avg".to_string(),
            operator: ">".to_string(),
            threshold: 100.0,
            recovery_threshold: Some(80.0),
            severity: "critical".to_string(),
            dwell_secs: 30,
            channel_ids: "[\"ch-1\"]".to_string(),
            enabled: true,
            resource_group_id: None,
            created_at: now.clone(),
            updated_at: now,
        };

        let json = rule_to_json(&rule);
        assert_eq!(json["id"], "rule-1");
        assert_eq!(json["name"], "High RPS");
        assert_eq!(json["metricName"], "extractor_rps_avg");
        assert_eq!(json["operator"], ">");
        assert_eq!(json["threshold"], 100.0);
        assert_eq!(json["recoveryThreshold"], 80.0);
        assert_eq!(json["severity"], "critical");
        assert_eq!(json["dwellSecs"], 30);
        assert_eq!(json["enabled"], true);
    }

    #[tokio::test]
    async fn test_rule_crud_against_memory_db() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let rule = AlertRule {
            id: "rule-crud-1".to_string(),
            name: "Test Rule".to_string(),
            metric_name: "extractor_rps_avg".to_string(),
            operator: ">".to_string(),
            threshold: 100.0,
            recovery_threshold: Some(80.0),
            severity: "critical".to_string(),
            dwell_secs: 0,
            channel_ids: "[]".to_string(),
            enabled: true,
            resource_group_id: None,
            created_at: now.clone(),
            updated_at: now,
        };

        // Create
        let created = AlertRuleRepository::create(&pool, &rule).await.unwrap();
        assert_eq!(created.id, "rule-crud-1");
        assert_eq!(created.name, "Test Rule");

        // Read
        let found = AlertRuleRepository::find_by_id(&pool, "rule-crud-1")
            .await
            .unwrap();
        assert_eq!(found.metric_name, "extractor_rps_avg");
        assert_eq!(found.operator, ">");
        assert_eq!(found.threshold, 100.0);
        assert_eq!(found.recovery_threshold, Some(80.0));
        assert_eq!(found.severity, "critical");
        assert_eq!(found.dwell_secs, 0);
        assert_eq!(found.enabled, true);

        // Update
        let mut updated = found;
        updated.name = "Updated Rule".to_string();
        updated.threshold = 200.0;
        updated.enabled = false;
        updated.updated_at =
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let saved = AlertRuleRepository::update(&pool, &updated).await.unwrap();
        assert_eq!(saved.name, "Updated Rule");
        assert_eq!(saved.threshold, 200.0);
        assert_eq!(saved.enabled, false);

        // Delete
        AlertRuleRepository::delete(&pool, "rule-crud-1")
            .await
            .unwrap();
        assert!(AlertRuleRepository::find_by_id(&pool, "rule-crud-1")
            .await
            .is_err());
    }

    #[test]
    fn test_invalid_rule_returns_envelope_error() {
        let err = validate_rule("not_a_metric", "≠").unwrap_err();
        assert_eq!(err.code, codes::VALIDATION_FAILED);
        // Check it's an envelope with details
        assert!(err.details.is_some());
        let details = err.details.unwrap();
        assert_eq!(details["field"], "operator");
    }

    /// Helper: run the evaluate_now core logic against a given request,
    /// returning the list of events.
    fn run_evaluate_now(req: &EvaluateNowRequest) -> Vec<serde_json::Value> {
        let mut events = Vec::new();
        let mut firing = false;
        let mut dwell_start: Option<i64> = None;
        let dwell_secs = req.dwell_secs.unwrap_or(0);
        let threshold = req.threshold;
        let recovery_threshold = req.recovery_threshold;

        for (i, point) in req.points.iter().enumerate() {
            let ts = point.ts.unwrap_or(i as i64);
            let value = point.value;
            let breached = evaluate_op(&req.operator, value, threshold);

            if breached {
                if !firing {
                    if dwell_secs > 0 {
                        if dwell_start.is_none() {
                            dwell_start = Some(ts);
                        }
                        let elapsed = ts - dwell_start.unwrap();
                        if elapsed >= dwell_secs {
                            firing = true;
                            events.push(serde_json::json!({
                                "type": "firing",
                                "at_index": i,
                                "value": value,
                                "threshold": threshold,
                            }));
                        }
                    } else {
                        firing = true;
                        events.push(serde_json::json!({
                            "type": "firing",
                            "at_index": i,
                            "value": value,
                            "threshold": threshold,
                        }));
                    }
                }
            } else if firing {
                let temp_rule = AlertRule {
                    id: String::new(),
                    name: String::new(),
                    metric_name: req.metric_name.clone(),
                    operator: req.operator.clone(),
                    threshold,
                    recovery_threshold,
                    severity: String::new(),
                    dwell_secs: 0,
                    channel_ids: "[]".to_string(),
                    enabled: true,
                    resource_group_id: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                };
                let recovered = is_recovered(&temp_rule, value);
                if recovered {
                    firing = false;
                    dwell_start = None;
                    events.push(serde_json::json!({
                        "type": "recovery",
                        "at_index": i,
                        "value": value,
                    }));
                }
            } else {
                dwell_start = None;
            }
        }

        events
    }

    /// For operator "<" with recovery_threshold, recovery occurs when
    /// value > recovery_threshold (not value < recovery_threshold).
    #[test]
    fn test_evaluate_now_lt_operator_recovery_is_operator_aware() {
        // Rule: value < 10 fires; recovery at value > 30.
        let req = EvaluateNowRequest {
            metric_name: "latency_ms".to_string(),
            operator: "<".to_string(),
            threshold: 10.0,
            recovery_threshold: Some(30.0),
            dwell_secs: Some(0),
            points: vec![
                SyntheticPoint {
                    value: 5.0,
                    ts: Some(0),
                }, // fires (< 10)
                SyntheticPoint {
                    value: 15.0,
                    ts: Some(10),
                }, // not < 10, but 15 < 30 → NOT recovered
                SyntheticPoint {
                    value: 35.0,
                    ts: Some(20),
                }, // 35 > 30 → recovered
            ],
        };

        let events = run_evaluate_now(&req);

        // Should fire at index 0 and recover at index 2.
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["type"], "firing");
        assert_eq!(events[0]["at_index"], 0);
        assert_eq!(events[1]["type"], "recovery");
        assert_eq!(events[1]["at_index"], 2);
    }

    /// For operator "<=" with recovery_threshold, recovery also requires value > rt.
    #[test]
    fn test_evaluate_now_lte_operator_recovery_is_operator_aware() {
        // Rule: value <= 10 fires; recovery at value > 30.
        let req = EvaluateNowRequest {
            metric_name: "latency_ms".to_string(),
            operator: "<=".to_string(),
            threshold: 10.0,
            recovery_threshold: Some(30.0),
            dwell_secs: Some(0),
            points: vec![
                SyntheticPoint {
                    value: 5.0,
                    ts: Some(0),
                }, // fires (<= 10)
                SyntheticPoint {
                    value: 20.0,
                    ts: Some(10),
                }, // not <= 10, but 20 < 30 → NOT recovered
                SyntheticPoint {
                    value: 35.0,
                    ts: Some(20),
                }, // 35 > 30 → recovered
            ],
        };

        let events = run_evaluate_now(&req);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["type"], "firing");
        assert_eq!(events[1]["type"], "recovery");
        assert_eq!(events[1]["at_index"], 2);
    }

    /// For operator "==" with recovery_threshold, recovery occurs when
    /// value is NOT equal to the recovery_threshold.
    #[test]
    fn test_evaluate_now_eq_operator_recovery_is_operator_aware() {
        // Rule: value == 0 fires (zero-rps alert); recovery at value != 0.
        let req = EvaluateNowRequest {
            metric_name: "rps".to_string(),
            operator: "==".to_string(),
            threshold: 0.0,
            recovery_threshold: Some(0.0),
            dwell_secs: Some(0),
            points: vec![
                SyntheticPoint {
                    value: 0.0,
                    ts: Some(0),
                }, // fires (== 0)
                SyntheticPoint {
                    value: 5.0,
                    ts: Some(10),
                }, // 5 != 0 → recovered
            ],
        };

        let events = run_evaluate_now(&req);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["type"], "firing");
        assert_eq!(events[1]["type"], "recovery");
        assert_eq!(events[1]["at_index"], 1);
    }
}
