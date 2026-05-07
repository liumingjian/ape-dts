//! Alert API handlers: query, clear, batch clear, SSE stream, test channel.
//!
//! - GET  /api/alerts?status=&level=&engine=&taskId= — filtered list
//! - POST /api/alerts/:id/clear — clear a single alert
//! - POST /api/alerts/clear_batch — batch clear (atomic per request)
//! - GET  /api/alerts/stream — SSE alert stream (firing/recovery/cleared)
//! - POST /api/alarm_channels/:id/test — synthetic test firing
//!
//! Already-cleared re-clear is idempotent.
//! Batch lifecycle reports per-row success/failure.
//! XSS prevention: user-supplied text escaped at render time.

use actix_web::{get, post, web, HttpRequest, HttpResponse, Responder, ResponseError};
use actix_web_lab::sse::{Data, Event as SseEvent, Sse};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::alarm_dispatcher;
use crate::alert_engine::AlertEvent;
use crate::error::{codes, ApiError};
use crate::idempotency::{extract_key, IdempotencyCache};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{Alert, UserContext};
use crate::repositories::alarm_channel_repository::AlarmChannelRepository;
use crate::repositories::alert_repository::AlertRepository;
use crate::sse_session_tracker::SseSessionTracker;

/// Default page size for alert listing.
const DEFAULT_PAGE_SIZE: i64 = 20;

/// SSE retry hint in milliseconds.
const SSE_RETRY_HINT_MS: u64 = 5000;

/// SSE heartbeat interval in seconds.
const SSE_HEARTBEAT_INTERVAL_SECS: u64 = 30;

/// Shared state for the alert SSE broadcaster.
#[derive(Debug, Clone, Default)]
pub struct AlertSseState {
    /// Broadcast sender for alert events.
    tx: Arc<Mutex<Option<tokio::sync::broadcast::Sender<AlertSseEvent>>>>,
}

/// An SSE event for the alert stream.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AlertSseEvent {
    Firing {
        id: String,
        task_id: Option<String>,
        run_id: Option<String>,
        rule_id: Option<String>,
        severity: String,
        metric: Option<String>,
        value: Option<f64>,
        threshold: Option<f64>,
        fired_at: String,
        silenced: bool,
    },
    Recovery {
        id: String,
        task_id: Option<String>,
        run_id: Option<String>,
        severity: String,
        recovered_at: String,
        /// The status the alert held before recovery (always "firing" per VAL-SSE-008).
        previous_status: String,
    },
    Cleared {
        id: String,
        cleared_by: Option<String>,
    },
}

const BROADCAST_CAPACITY: usize = 256;

impl AlertSseState {
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(BROADCAST_CAPACITY);
        Self {
            tx: Arc::new(Mutex::new(Some(tx))),
        }
    }

    /// Broadcast an alert event to all SSE subscribers.
    pub async fn broadcast(&self, event: AlertSseEvent) {
        let tx = self.tx.lock().await;
        if let Some(ref sender) = *tx {
            let _ = sender.send(event);
        }
    }
}

/// Publish an AlertEngine event to the SSE stream.
pub async fn publish_alert_event(state: &AlertSseState, event: &AlertEvent) {
    let sse_event = match event {
        AlertEvent::Firing {
            id,
            task_id,
            run_id,
            rule_id,
            severity,
            metric,
            value,
            threshold,
            fired_at,
            silenced,
            ..
        } => AlertSseEvent::Firing {
            id: id.clone(),
            task_id: task_id.clone(),
            run_id: run_id.clone(),
            rule_id: rule_id.clone(),
            severity: severity.clone(),
            metric: metric.clone(),
            value: *value,
            threshold: *threshold,
            fired_at: fired_at.clone(),
            silenced: *silenced,
        },
        AlertEvent::Recovery {
            id,
            task_id,
            run_id,
            severity,
            recovered_at,
            previous_status,
        } => AlertSseEvent::Recovery {
            id: id.clone(),
            task_id: task_id.clone(),
            run_id: run_id.clone(),
            severity: severity.clone(),
            recovered_at: recovered_at.clone(),
            previous_status: previous_status.clone(),
        },
        AlertEvent::CdcStalled {
            id,
            task_id,
            run_id,
            severity,
            fired_at,
            silenced,
            ..
        } => AlertSseEvent::Firing {
            id: id.clone(),
            task_id: Some(task_id.clone()),
            run_id: run_id.clone(),
            rule_id: None,
            severity: severity.clone(),
            metric: Some("cdc_stalled".to_string()),
            value: None,
            threshold: None,
            fired_at: fired_at.clone(),
            silenced: *silenced,
        },
        AlertEvent::CdcRecovered {
            id,
            task_id,
            recovered_at,
        } => AlertSseEvent::Recovery {
            id: id.clone(),
            task_id: Some(task_id.clone()),
            run_id: None,
            severity: "critical".to_string(),
            recovered_at: recovered_at.clone(),
            previous_status: "firing".to_string(),
        },
    };
    state.broadcast(sse_event).await;
}

/// GET /api/alerts — list alerts with optional filters.
///
/// Query parameters:
/// - `status` — filter by alert status (firing, recovered, cleared)
/// - `level` — filter by severity (info, warning, critical)
/// - `engine` — filter by engine type (via task join)
/// - `taskId` — filter by task ID
/// - `page` — page number (default 1)
/// - `pageSize` — items per page (default 20)
#[get("/alerts")]
pub async fn list_alerts(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    query: web::Query<AlertListQuery>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(DEFAULT_PAGE_SIZE).max(1);

    match AlertRepository::list_filtered(
        &pool,
        query.status.as_deref(),
        query.level.as_deref(),
        query.engine.as_deref(),
        query.task_id.as_deref(),
        page,
        page_size,
    )
    .await
    {
        Ok((items, total)) => {
            let items_json: Vec<serde_json::Value> = items.iter().map(alert_to_json).collect();
            HttpResponse::Ok().json(serde_json::json!({
                "items": items_json,
                "total": total,
                "page": page,
                "pageSize": page_size,
            }))
        }
        Err(e) => {
            tracing::warn!("alert list query failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to list alerts").error_response()
        }
    }
}

/// POST /api/alerts/:id/clear — clear a single alert.
///
/// Sets status=cleared, cleared_at=now, cleared_by=session_user.
/// Emits one SSE `event: cleared`.
/// Already-cleared re-clear is idempotent.
/// Honours Idempotency-Key: replayed key returns cached result.
#[post("/alerts/{id}/clear")]
pub async fn clear_alert(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    sse_state: web::Data<AlertSseState>,
    idempotency_cache: web::Data<IdempotencyCache>,
    req: HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlertClear) {
        return e.error_response();
    }

    // Idempotency-Key check: if the key was seen before, return the cached result.
    let idem_key = extract_key(&req);
    if let Some(ref key) = idem_key {
        if let Some(cached) = idempotency_cache.get(key).await {
            return HttpResponse::build(
                actix_web::http::StatusCode::from_u16(cached.status)
                    .unwrap_or(actix_web::http::StatusCode::OK),
            )
            .json(cached.body);
        }
    }

    let alert_id = path.into_inner();

    let alert = match AlertRepository::find_by_id(&pool, &alert_id).await {
        Ok(a) => a,
        Err(_) => {
            return ApiError::with_details(
                codes::NOT_FOUND,
                "Alert not found",
                serde_json::json!({ "id": alert_id }),
            )
            .error_response();
        }
    };

    // Idempotent: already-cleared → return 200 with noop=true.
    if alert.status == "cleared" {
        let already_cleared_body = serde_json::json!({
            "id": alert.id,
            "status": "cleared",
            "cleared_at": alert.cleared_at,
            "cleared_by": alert.cleared_by,
            "noop": true,
        });
        if let Some(ref key) = idem_key {
            idempotency_cache
                .put(key, 200, already_cleared_body.clone())
                .await;
        }
        return HttpResponse::Ok().json(already_cleared_body);
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut updated = alert;
    updated.status = "cleared".to_string();
    updated.cleared_at = Some(now.clone());
    updated.cleared_by = Some(user.username.clone());

    match AlertRepository::update(&pool, &updated).await {
        Ok(persisted) => {
            // Publish cleared event to SSE stream.
            sse_state
                .broadcast(AlertSseEvent::Cleared {
                    id: persisted.id.clone(),
                    cleared_by: Some(user.username.clone()),
                })
                .await;

            let cleared_body = serde_json::json!({
                "id": persisted.id,
                "status": "cleared",
                "cleared_at": persisted.cleared_at,
                "cleared_by": persisted.cleared_by,
            });
            if let Some(ref key) = idem_key {
                idempotency_cache.put(key, 200, cleared_body.clone()).await;
            }
            HttpResponse::Ok().json(cleared_body)
        }
        Err(e) => {
            tracing::warn!("alert clear update failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to clear alert").error_response()
        }
    }
}

/// POST /api/alerts/clear_batch — batch clear alerts.
///
/// Atomic per request: each id is cleared independently.
/// Returns per-row success/failure outcome.
/// Honours Idempotency-Key: replayed key returns cached result.
#[post("/alerts/clear_batch")]
pub async fn clear_batch(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    body: web::Json<ClearBatchRequest>,
    sse_state: web::Data<AlertSseState>,
    idempotency_cache: web::Data<IdempotencyCache>,
    req: HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlertClear) {
        return e.error_response();
    }

    // Idempotency-Key check: if the key was seen before, return the cached result.
    let idem_key = extract_key(&req);
    if let Some(ref key) = idem_key {
        if let Some(cached) = idempotency_cache.get(key).await {
            return HttpResponse::build(
                actix_web::http::StatusCode::from_u16(cached.status)
                    .unwrap_or(actix_web::http::StatusCode::OK),
            )
            .json(cached.body);
        }
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut successes = Vec::new();
    let mut failures = Vec::new();

    for id in &body.ids {
        match AlertRepository::find_by_id(&pool, id).await {
            Ok(alert) => {
                if alert.status == "cleared" {
                    // Idempotent: already cleared.
                    successes.push(serde_json::json!({
                        "id": id,
                        "status": "cleared",
                        "noop": true,
                    }));
                } else {
                    let mut updated = alert;
                    updated.status = "cleared".to_string();
                    updated.cleared_at = Some(now.clone());
                    updated.cleared_by = Some(user.username.clone());

                    match AlertRepository::update(&pool, &updated).await {
                        Ok(persisted) => {
                            sse_state
                                .broadcast(AlertSseEvent::Cleared {
                                    id: persisted.id.clone(),
                                    cleared_by: Some(user.username.clone()),
                                })
                                .await;

                            successes.push(serde_json::json!({
                                "id": persisted.id,
                                "status": "cleared",
                            }));
                        }
                        Err(e) => {
                            failures.push(serde_json::json!({
                                "id": id,
                                "code": "INTERNAL_ERROR",
                                "message": e.to_string(),
                            }));
                        }
                    }
                }
            }
            Err(_) => {
                failures.push(serde_json::json!({
                    "id": id,
                    "code": "NOT_FOUND",
                    "message": "Alert not found",
                }));
            }
        }
    }

    let result_body = serde_json::json!({
        "successes": successes,
        "failures": failures,
    });

    // Cache the result if an Idempotency-Key was provided.
    if let Some(ref key) = idem_key {
        idempotency_cache.put(key, 200, result_body.clone()).await;
    }

    HttpResponse::Ok().json(result_body)
}

/// GET /api/alerts/stream — SSE alert stream.
///
/// Delivers firing/recovery/cleared events as JSON SSE.
#[get("/alerts/stream")]
pub async fn alert_stream(
    user: UserContext,
    sse_state: web::Data<AlertSseState>,
    sse_tracker: web::Data<SseSessionTracker>,
    session: actix_session::Session,
    req: HttpRequest,
) -> HttpResponse {
    // RBAC: viewer and above can subscribe
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<SseEvent>(100);

    // Register this SSE connection with the session tracker so it can be
    // closed when the session is invalidated (logout, expiry, disable).
    // The tracker returns a cancellation handle; the producer checks
    // its receiver to know when to stop.
    let session_token = session
        .get::<String>(crate::auth::SESSION_TOKEN_KEY)
        .ok()
        .flatten();
    let cancel_handle = if let Some(ref token) = session_token {
        Some(sse_tracker.register(token).await)
    } else {
        None
    };

    // Subscribe to the broadcast channel.
    let bcast_rx = {
        let tx = sse_state.tx.lock().await;
        match tx.as_ref() {
            Some(sender) => sender.subscribe(),
            None => {
                return ApiError::new(codes::INTERNAL_ERROR, "Alert stream not available")
                    .error_response();
            }
        }
    };

    // Spawn the stream producer.
    let producer_sse_tracker = sse_tracker.get_ref().clone();
    let producer_session_token = session_token.clone();
    let producer_cancel_rx = cancel_handle.as_ref().map(|h| h.receiver());
    tokio::spawn(async move {
        produce_alert_sse_events(bcast_rx, event_tx, producer_cancel_rx).await;

        // Unregister from the session tracker when the stream ends
        if let Some(ref token) = producer_session_token {
            producer_sse_tracker.unregister(token).await;
        }
    });

    let sse = Sse::from_infallible_receiver(event_rx)
        .with_retry_duration(Duration::from_millis(SSE_RETRY_HINT_MS))
        .with_keep_alive(Duration::from_secs(SSE_HEARTBEAT_INTERVAL_SECS));

    sse.respond_to(&req)
}

/// POST /api/alarm_channels/:id/test — produce a synthetic firing.
///
/// Creates a test alert routed only to the addressed channel.
/// Not persisted to the live alerts list.
#[post("/alarm_channels/{id}/test")]
pub async fn test_alarm_channel(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::AlertClear) {
        return e.error_response();
    }

    let channel_id = path.into_inner();

    let channel = match AlarmChannelRepository::find_by_id(&pool, &channel_id).await {
        Ok(ch) => ch,
        Err(_) => {
            return ApiError::with_details(
                codes::NOT_FOUND,
                "Alarm channel not found",
                serde_json::json!({ "id": channel_id }),
            )
            .error_response();
        }
    };

    let result = alarm_dispatcher::test_channel(&channel).await;

    HttpResponse::Ok().json(serde_json::json!({
        "channelId": channel.id,
        "channelKind": channel.kind,
        "result": result,
        "synthetic": true,
    }))
}

/// Convert an Alert to a JSON value, escaping user-supplied text for XSS prevention.
fn alert_to_json(alert: &Alert) -> serde_json::Value {
    serde_json::json!({
        "id": escape_xss(&alert.id),
        "taskId": alert.task_id.as_ref().map(|s| escape_xss(s)),
        "runId": alert.run_id.as_ref().map(|s| escape_xss(s)),
        "ruleId": alert.rule_id.as_ref().map(|s| escape_xss(s)),
        "metricName": alert.metric_name.as_ref().map(|s| escape_xss(s)),
        "operator": alert.operator.as_ref().map(|s| escape_xss(s)),
        "threshold": alert.threshold,
        "severity": escape_xss(&alert.severity),
        "value": alert.value,
        "status": escape_xss(&alert.status),
        "silenced": alert.silenced,
        "firedAt": escape_xss(&alert.fired_at),
        "recoveredAt": alert.recovered_at.as_ref().map(|s| escape_xss(s)),
        "clearedAt": alert.cleared_at.as_ref().map(|s| escape_xss(s)),
        "deliveredAt": alert.delivered_at.as_ref().map(|s| escape_xss(s)),
        "clearedBy": alert.cleared_by.as_ref().map(|s| escape_xss(s)),
        "lastError": alert.last_error.as_ref().map(|s| escape_xss(s)),
        "createdAt": escape_xss(&alert.created_at),
    })
}

/// Escape user-supplied text for XSS prevention.
///
/// Replaces HTML-special characters with their entity equivalents:
/// & → &amp;, < → &lt;, > → &gt;, " → &quot;, ' → &#x27;
pub fn escape_xss(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// Produce SSE events from the alert broadcast channel.
///
/// If a cancellation receiver is provided, the producer checks it on each
/// iteration and stops when cancellation is signalled (e.g. on logout).
async fn produce_alert_sse_events(
    mut bcast_rx: tokio::sync::broadcast::Receiver<AlertSseEvent>,
    event_tx: tokio::sync::mpsc::Sender<SseEvent>,
    cancel_rx: Option<tokio::sync::watch::Receiver<bool>>,
) {
    let mut event_id: u64 = 0;

    loop {
        // Check if the session was invalidated (logout, expiry, disable).
        if let Some(ref rx) = cancel_rx {
            if *rx.borrow() {
                break;
            }
        }

        match bcast_rx.recv().await {
            Ok(alert_event) => {
                event_id += 1;
                let event_type = match &alert_event {
                    AlertSseEvent::Firing { .. } => "firing",
                    AlertSseEvent::Recovery { .. } => "recovery",
                    AlertSseEvent::Cleared { .. } => "cleared",
                };
                let data = serde_json::to_string(&alert_event).unwrap_or_default();
                let sse_event =
                    SseEvent::Data(Data::new(data).id(event_id.to_string()).event(event_type));
                if event_tx.send(sse_event).await.is_err() {
                    break; // Client disconnected
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                // Missed events — continue
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                break;
            }
        }
    }
}

// ─── Request/Response Types ────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AlertListQuery {
    pub status: Option<String>,
    pub level: Option<String>,
    pub engine: Option<String>,
    pub task_id: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ClearBatchRequest {
    pub ids: Vec<String>,
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_xss_html_chars() {
        assert_eq!(
            escape_xss("<script>alert(1)</script>"),
            "&lt;script&gt;alert(1)&lt;/script&gt;"
        );
    }

    #[test]
    fn test_escape_xss_ampersand() {
        assert_eq!(escape_xss("a&b"), "a&amp;b");
    }

    #[test]
    fn test_escape_xss_quotes() {
        assert_eq!(escape_xss("a\"b'c"), "a&quot;b&#x27;c");
    }

    #[test]
    fn test_escape_xss_passthrough_safe() {
        assert_eq!(escape_xss("hello world"), "hello world");
        assert_eq!(escape_xss("task-123"), "task-123");
    }

    #[test]
    fn test_clear_batch_request_parse() {
        let req: ClearBatchRequest = serde_json::from_str(r#"{"ids":["a","b","c"]}"#).unwrap();
        assert_eq!(req.ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_alert_list_query_default_page() {
        let query: AlertListQuery = serde_urlencoded::from_str("").unwrap();
        assert_eq!(query.page, None);
        assert_eq!(query.page_size, None);
    }

    #[tokio::test]
    async fn test_sse_state_broadcast() {
        let state = AlertSseState::new();
        let mut rx = {
            let tx = state.tx.lock().await;
            tx.as_ref().unwrap().subscribe()
        };

        state
            .broadcast(AlertSseEvent::Cleared {
                id: "test-id".into(),
                cleared_by: Some("admin".into()),
            })
            .await;

        let event = rx.try_recv().unwrap();
        match event {
            AlertSseEvent::Cleared { id, cleared_by } => {
                assert_eq!(id, "test-id");
                assert_eq!(cleared_by, Some("admin".into()));
            }
            _ => panic!("expected Cleared event"),
        }
    }

    /// VAL-SSE-008: Recovery event must include previous_status field.
    #[tokio::test]
    async fn test_sse_recovery_event_has_previous_status() {
        let state = AlertSseState::new();
        let mut rx = {
            let tx = state.tx.lock().await;
            tx.as_ref().unwrap().subscribe()
        };

        state
            .broadcast(AlertSseEvent::Recovery {
                id: "alert-1".into(),
                task_id: Some("task-1".into()),
                run_id: None,
                severity: "warning".into(),
                recovered_at: "2026-05-07T00:00:00.000Z".into(),
                previous_status: "firing".into(),
            })
            .await;

        let event = rx.try_recv().unwrap();
        match event {
            AlertSseEvent::Recovery {
                id,
                previous_status,
                ..
            } => {
                assert_eq!(id, "alert-1");
                assert_eq!(
                    previous_status, "firing",
                    "Recovery event must include previous_status='firing' per VAL-SSE-008"
                );
            }
            _ => panic!("expected Recovery event"),
        }

        // Also verify JSON serialization includes previous_status
        let recovery = AlertSseEvent::Recovery {
            id: "a1".into(),
            task_id: None,
            run_id: None,
            severity: "warning".into(),
            recovered_at: "2026-05-07T00:00:00.000Z".into(),
            previous_status: "firing".into(),
        };
        let json = serde_json::to_string(&recovery).unwrap();
        assert!(
            json.contains("previous_status"),
            "Serialized Recovery must include previous_status field: {json}"
        );
    }
}
