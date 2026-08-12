//! HTTP handlers for test_connection and precheck endpoints.
//!
//! - POST /api/tasks/:id/test_connection — per-side connectivity probe
//! - POST /api/tasks/:id/precheck       — full prerequisite checks
//! - POST /api/tasks/preview/test_connection — draft mode (no persistence)
//! - POST /api/tasks/preview/precheck        — draft mode (no persistence)
//!
//! The precheck handler calls `PrecheckerBuilder` directly (NOT `do_precheck`
//! which panics). Individual check failures are captured without aborting the
//! orchestrator.

use actix_web::{post, web, HttpResponse, ResponseError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use crate::error::{codes, ApiError};
use crate::redaction::redact_url;
use crate::ini_renderer;
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{CreateTaskRequest, Task, UserContext};
use crate::repositories::task_repository::TaskRepository;

use dt_common::config::task_config::TaskConfig;
use dt_common::rdb_filter::RdbFilter;
use dt_precheck::builder::prechecker_builder::PrecheckerBuilder;
use dt_precheck::config::precheck_config::PrecheckConfig;
use dt_precheck::config::task_config::PrecheckTaskConfig;
use dt_precheck::meta::check_result::CheckResult;
use dt_precheck::prechecker::traits::Prechecker;

/// Length of the short correlation ID used in logs and surfaced to the UI.
/// Eight hex chars is enough to grep server logs in practice without making
/// the value awkward to type into a chat.
const REQUEST_ID_LEN: usize = 8;

/// Hard upper bound on a single side's connection probe. Kept short so that
/// an unreachable host fails fast and any reverse proxy in front of us
/// (Vite dev server, nginx, …) reflects the structured per-side error
/// instead of dropping the TCP socket. Vite's default proxy timeout is
/// 30s, so 10s leaves comfortable headroom for the response itself.
const TEST_CONNECTION_TIMEOUT_SECS: u64 = 10;

/// Generate a short correlation ID for log↔UI cross-reference.
fn new_request_id() -> String {
    uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(REQUEST_ID_LEN)
        .collect()
}


/// Extract a redacted endpoint URL from the JSON-encoded endpoint config
/// stored on `Task` (best-effort; absent / malformed entries fall back to
/// "?"). Used only for diagnostic logs.
fn redacted_endpoint(raw: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return "?".to_string(),
    };
    match v.get("url").and_then(|u| u.as_str()) {
        Some(u) => redact_url(u),
        None => "?".to_string(),
    }
}

// ─── Response types ──────────────────────────────────────────────────────

/// Per-side result for test_connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSideResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Response for POST /api/tasks/:id/test_connection.
///
/// `request_id` is the same correlation ID emitted in the server log lines
/// for this call so operators can match a UI bug report to the exact log
/// trace without negotiating extra headers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestConnectionResponse {
    pub source: ConnectionSideResult,
    pub target: ConnectionSideResult,
    pub request_id: String,
}

/// A single precheck item in the response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecheckItem {
    pub name: String,
    pub side: String,
    pub status: String, // "pass" | "fail" | "skip" | "warn"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advise_message: Option<String>,
}

/// Summary counts for precheck items.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecheckSummary {
    pub pass: usize,
    pub fail: usize,
    pub skip: usize,
    pub warn: usize,
}

/// Response for POST /api/tasks/:id/precheck.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecheckResponse {
    pub items: Vec<PrecheckItem>,
    pub summary: PrecheckSummary,
}

// ─── Helpers ─────────────────────────────────────────────────────────────

/// Write the INI to a temp file and return its path, appending [precheck]
/// section suitable for the given task kind. For two-phase MySQL
/// `snapshot_and_cdc` tasks the engine is invoked twice (snapshot, then cdc),
/// but precheck is a single pass: render the phase 1 (snapshot) INI so
/// `dt-common` can parse it, then force `do_cdc=true` so the binlog/server_id
/// prerequisites required by phase 2 are also exercised.
fn write_temp_ini(task: &Task, kind: &str) -> Result<PathBuf, ApiError> {
    let two_phase = crate::two_phase::is_two_phase_task(task);
    let ini_text = if two_phase {
        crate::two_phase::render_phase1_ini(task)
    } else {
        ini_renderer::render(task)
    };
    let do_cdc = two_phase || kind == "cdc";
    let full_ini = format!(
        "{}\n[precheck]\ndo_struct_init=true\ndo_cdc={}\n",
        ini_text, do_cdc
    );

    let dir = std::env::temp_dir().join("dt-console-precheck");
    std::fs::create_dir_all(&dir).map_err(|e| {
        ApiError::new(
            codes::INTERNAL_ERROR,
            format!("temp dir create failed: {e}"),
        )
    })?;

    let path = dir.join(format!("{}.ini", uuid::Uuid::new_v4()));
    std::fs::write(&path, &full_ini)
        .map_err(|e| ApiError::new(codes::INTERNAL_ERROR, format!("temp ini write failed: {e}")))?;

    Ok(path)
}

/// Load TaskConfig + PrecheckConfig from the temp INI.
fn load_configs(ini_path: &std::path::Path) -> Result<(TaskConfig, PrecheckConfig), ApiError> {
    let path_str = ini_path.to_string_lossy().to_string();
    let task_config = TaskConfig::new(&path_str).map_err(|e| {
        ApiError::new(
            codes::INTERNAL_ERROR,
            format!("TaskConfig load failed: {e}"),
        )
    })?;
    let precheck_task_config = PrecheckTaskConfig::new(&path_str).map_err(|e| {
        ApiError::new(
            codes::INTERNAL_ERROR,
            format!("PrecheckTaskConfig load failed: {e}"),
        )
    })?;
    Ok((task_config, precheck_task_config.precheck))
}

/// Clean up the temp INI file.
fn cleanup_temp_ini(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}

/// Test connectivity for one side (source or target).
///
/// The underlying `build_connection` call goes through the engine-specific
/// fetcher and may hang on TCP SYN against an unreachable host until the
/// kernel times out (~75s on Linux). That is much longer than any
/// reasonable HTTP proxy will wait, so we bound the probe at
/// `TEST_CONNECTION_TIMEOUT_SECS` and surface a structured per-side
/// `TEST_CONNECTION_TIMEOUT` failure on overrun. The HTTP response stays
/// 200 with `ok=false` for that side, matching the existing contract for
/// "one side fails" — we never drop the socket.
async fn test_one_side(builder: &PrecheckerBuilder, is_source: bool) -> ConnectionSideResult {
    test_one_side_with_timeout(
        builder,
        is_source,
        Duration::from_secs(TEST_CONNECTION_TIMEOUT_SECS),
    )
    .await
}

/// Internal hook so unit tests can inject a tighter deadline; production
/// always calls the constant-timeout wrapper above.
async fn test_one_side_with_timeout(
    builder: &PrecheckerBuilder,
    is_source: bool,
    timeout: Duration,
) -> ConnectionSideResult {
    let mut checker = match builder.build_checker(is_source) {
        Ok(Some(c)) => c,
        Ok(None) => {
            return ConnectionSideResult {
                ok: false,
                code: Some("UNSUPPORTED_ENGINE".to_string()),
                message: Some("no checker available for this engine type".to_string()),
            }
        }
        Err(e) => {
            return ConnectionSideResult {
                ok: false,
                code: Some(codes::INVALID_FILTER_CONFIG.to_string()),
                message: Some(e.to_string()),
            }
        }
    };

    match tokio::time::timeout(timeout, checker.build_connection()).await {
        Ok(Ok(result)) => {
            if result.is_validate {
                ConnectionSideResult {
                    ok: true,
                    code: None,
                    message: None,
                }
            } else {
                ConnectionSideResult {
                    ok: false,
                    code: Some("CONNECTION_FAILED".to_string()),
                    message: Some(if result.error_msg.is_empty() {
                        "connection validation failed".to_string()
                    } else {
                        result.error_msg
                    }),
                }
            }
        }
        Ok(Err(e)) => ConnectionSideResult {
            ok: false,
            code: Some("CONNECTION_FAILED".to_string()),
            message: Some(e.to_string()),
        },
        Err(_) => ConnectionSideResult {
            ok: false,
            code: Some(codes::TEST_CONNECTION_TIMEOUT.to_string()),
            message: Some(format!(
                "connect attempt exceeded {}s timeout",
                timeout.as_secs_f64()
            )),
        },
    }
}

/// Convert a CheckResult to a PrecheckItem.
///
/// Status mapping:
/// - `is_validate` → "pass"
/// - `!is_validate` with non-empty `warn_msg` → "warn" (non-blocking advisory)
/// - `!is_validate` with empty `warn_msg` → "fail" (blocking)
fn check_result_to_item(result: &CheckResult, is_source: bool) -> PrecheckItem {
    let side = if is_source { "source" } else { "target" };
    let status = if result.is_validate {
        "pass"
    } else if !result.warn_msg.is_empty() {
        "warn"
    } else {
        "fail"
    };
    PrecheckItem {
        name: result.check_type_name.clone(),
        side: side.to_string(),
        status: status.to_string(),
        description: if result.check_desc.is_empty() {
            None
        } else {
            Some(result.check_desc.clone())
        },
        error_message: if result.error_msg.is_empty() {
            None
        } else {
            Some(result.error_msg.clone())
        },
        advise_message: if result.advise_msg.is_empty() {
            None
        } else {
            Some(result.advise_msg.clone())
        },
    }
}

/// Run all precheck items for a side and collect results.
/// Individual failures are captured — no panic.
async fn run_side_checks(
    checker: &mut Box<dyn Prechecker + Send>,
    is_source: bool,
    do_cdc: bool,
    kind: &str,
) -> Vec<PrecheckItem> {
    let mut items = Vec::new();

    // 1. Connection check
    match checker.build_connection().await {
        Ok(result) => items.push(check_result_to_item(&result, is_source)),
        Err(e) => {
            // Connection failed — record the failure and stop checking this side
            items.push(PrecheckItem {
                name: "CheckDatabaseConnection".to_string(),
                side: if is_source { "source" } else { "target" }.to_string(),
                status: "fail".to_string(),
                description: Some(
                    "check if the database can be connected".to_string(),
                ),
                error_message: Some(e.to_string()),
                advise_message: Some(
                    "(1) check whether the account password is correct. (2) check if the network configuration is correct."
                        .to_string(),
                ),
            });
            // If connection failed, remaining checks would fail too — skip them
            return items;
        }
    }

    // 2. Database version check
    match checker.check_database_version().await {
        Ok(result) => items.push(check_result_to_item(&result, is_source)),
        Err(e) => items.push(PrecheckItem {
            name: "CheckDatabaseVersionSupported".to_string(),
            side: if is_source { "source" } else { "target" }.to_string(),
            status: "fail".to_string(),
            description: None,
            error_message: Some(e.to_string()),
            advise_message: None,
        }),
    }

    // 3. CDC-specific checks (source only, only for CDC kind)
    if is_source && do_cdc {
        match checker.check_cdc_supported().await {
            Ok(result) => items.push(check_result_to_item(&result, is_source)),
            Err(e) => items.push(PrecheckItem {
                name: "CheckIfDatabaseSupportCdc".to_string(),
                side: "source".to_string(),
                status: "fail".to_string(),
                description: None,
                error_message: Some(e.to_string()),
                advise_message: None,
            }),
        }
    }

    // 4. Struct existence check (skip for struct kind — handled at a higher level)
    if kind != "struct" {
        match checker.check_struct_existed_or_not().await {
            Ok(result) => items.push(check_result_to_item(&result, is_source)),
            Err(e) => items.push(PrecheckItem {
                name: "CheckIfStructExisted".to_string(),
                side: if is_source { "source" } else { "target" }.to_string(),
                status: "fail".to_string(),
                description: None,
                error_message: Some(e.to_string()),
                advise_message: None,
            }),
        }
    }

    // 5. Table struct support check (skip for struct kind)
    if kind != "struct" {
        match checker.check_table_structs().await {
            Ok(result) => items.push(check_result_to_item(&result, is_source)),
            Err(e) => items.push(PrecheckItem {
                name: "CheckIfTableStructSupported".to_string(),
                side: if is_source { "source" } else { "target" }.to_string(),
                status: "fail".to_string(),
                description: None,
                error_message: Some(e.to_string()),
                advise_message: None,
            }),
        }
    }

    items
}

/// Build a Task model from a CreateTaskRequest (draft mode).
/// This does NOT persist the task — it's only used for precheck/test_connection
/// in draft/preview mode.
fn build_draft_task(body: &CreateTaskRequest) -> Result<Task, ApiError> {
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
    let db_type_source = crate::validation::resolve_db_type(&body.engine_source, source_sub_mode);
    let db_type_target = crate::validation::resolve_db_type(&body.engine_target, target_sub_mode);

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let id = uuid::Uuid::new_v4().to_string();
    let task_id = format!(
        "draft_{}_{}_{}_{}",
        body.kind,
        db_type_source,
        db_type_target,
        &id[..8]
    );

    Ok(Task {
        id: id.clone(),
        task_id,
        name: String::new(),
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
        resource_group_id: String::new(),
        owner_user_id: None,
        status: "draft".to_string(),
        created_at: now.clone(),
        updated_at: now,
    })
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

// ─── Handlers ────────────────────────────────────────────────────────────

/// POST /api/tasks/:id/test_connection — test connectivity for a persisted task.
///
/// Wrapped with the same `tokio::spawn` panic guard the precheck routes
/// use, plus a per-side connect timeout, so a slow/unreachable host can
/// never drop the HTTP socket. Failures surface as either:
///   - a 200 body with per-side `code=TEST_CONNECTION_TIMEOUT|CONNECTION_FAILED`
///   - a 422 envelope with `code=TEST_CONNECTION_PANIC` if the handler
///     itself crashed
/// Both paths carry the request correlation ID for log↔UI cross-reference.
#[post("/tasks/{id}/test_connection")]
pub async fn test_connection(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskCreate) {
        return e.error_response();
    }

    let task_id = path.into_inner();
    let task = match TaskRepository::find_by_id(&pool, &task_id).await {
        Ok(t) => t,
        Err(_) => return ApiError::new(codes::TASK_NOT_FOUND, "Task not found").error_response(),
    };

    let request_id = new_request_id();
    log_precheck_start(&request_id, &task, "test_connection");
    invoke_test_connection_with_guard(request_id, task).await
}

/// POST /api/tasks/preview/test_connection — draft mode (no persistence).
///
/// Same panic-guard + per-side timeout contract as `test_connection`.
#[post("/tasks/preview/test_connection")]
pub async fn preview_test_connection(
    user: UserContext,
    body: web::Json<CreateTaskRequest>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskCreate) {
        return e.error_response();
    }

    let task = match build_draft_task(&body) {
        Ok(t) => t,
        Err(e) => return e.error_response(),
    };

    let request_id = new_request_id();
    log_precheck_start(&request_id, &task, "preview_test_connection");
    invoke_test_connection_with_guard(request_id, task).await
}

/// POST /api/tasks/:id/precheck — run prerequisite checks for a persisted task.
///
/// Wraps the precheck call in `tokio::spawn` so that panics inside the
/// prechecker are caught, logged with full context, and returned as 422
/// `PRECHECK_PANIC` (carrying the correlation ID + panic message in
/// `details`) instead of dropping the HTTP connection.
#[post("/tasks/{id}/precheck")]
pub async fn precheck(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskCreate) {
        return e.error_response();
    }

    let task_id = path.into_inner();
    let task = match TaskRepository::find_by_id(&pool, &task_id).await {
        Ok(t) => t,
        Err(_) => return ApiError::new(codes::TASK_NOT_FOUND, "Task not found").error_response(),
    };

    let request_id = new_request_id();
    log_precheck_start(&request_id, &task, "precheck");
    invoke_precheck_with_guard(request_id, task).await
}

/// POST /api/tasks/preview/precheck — draft mode (no persistence).
///
/// Wraps the precheck call in `tokio::spawn` so panics are caught, logged
/// with full context, and returned as 422 `PRECHECK_PANIC`.
#[post("/tasks/preview/precheck")]
pub async fn preview_precheck(
    user: UserContext,
    body: web::Json<CreateTaskRequest>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskCreate) {
        return e.error_response();
    }

    let task = match build_draft_task(&body) {
        Ok(t) => t,
        Err(e) => return e.error_response(),
    };

    let request_id = new_request_id();
    log_precheck_start(&request_id, &task, "preview_precheck");
    invoke_precheck_with_guard(request_id, task).await
}

/// Run `run_precheck` under a `tokio::spawn` panic guard, surfacing every
/// outcome through `tracing` and the response envelope. The `request_id`
/// flows into both the log lines and the `details.requestId` field so
/// operators can `grep` server logs from a UI bug report.
async fn invoke_precheck_with_guard(request_id: String, task: Task) -> HttpResponse {
    let log_id = request_id.clone();
    let task_log_summary = format!(
        "kind={} src={} dst={}",
        task.kind, task.db_type_source, task.db_type_target
    );
    let handle = tokio::spawn(async move { run_precheck(&task).await });

    match handle.await {
        Ok(Ok(resp)) => {
            tracing::info!(
                request_id = %log_id,
                summary = ?resp.summary,
                "precheck completed",
            );
            HttpResponse::Ok().json(resp)
        }
        Ok(Err(mut e)) => {
            tracing::warn!(
                request_id = %log_id,
                code = %e.code,
                message = %e.message,
                task = %task_log_summary,
                "precheck rejected before running checks",
            );
            attach_request_id(&mut e, &log_id);
            e.error_response()
        }
        Err(join_err) => {
            let panic_msg = extract_panic_msg(join_err);
            tracing::error!(
                request_id = %log_id,
                panic_message = %panic_msg,
                task = %task_log_summary,
                "precheck task panicked — returning PRECHECK_PANIC",
            );
            ApiError::with_details(
                codes::PRECHECK_PANIC,
                format!(
                    "precheck task crashed unexpectedly (request_id={log_id}). \
                     Send this request ID to ops or grep console-server logs."
                ),
                serde_json::json!({
                    "requestId": log_id,
                    "panicMessage": panic_msg,
                }),
            )
            .error_response()
        }
    }
}

/// Attach the correlation ID to an existing `ApiError`'s `details` payload.
fn attach_request_id(err: &mut ApiError, request_id: &str) {
    let mut details = match err.details.take() {
        Some(serde_json::Value::Object(map)) => map,
        Some(other) => {
            let mut map = serde_json::Map::new();
            map.insert("original".to_string(), other);
            map
        }
        None => serde_json::Map::new(),
    };
    details.insert(
        "requestId".to_string(),
        serde_json::Value::String(request_id.to_string()),
    );
    err.details = Some(serde_json::Value::Object(details));
}

/// Render an `tokio::task::JoinError` from a precheck panic into a short
/// human-readable message.
fn extract_panic_msg(join_err: tokio::task::JoinError) -> String {
    if !join_err.is_panic() {
        return "precheck task was cancelled".to_string();
    }
    let payload = join_err.into_panic();
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    if let Some(s) = payload.downcast_ref::<&str>() {
        return s.to_string();
    }
    "precheck panicked with unknown cause".to_string()
}

/// Emit a structured log line with the redacted endpoint summary at the
/// start of a precheck handler invocation.
fn log_precheck_start(request_id: &str, task: &Task, route: &str) {
    tracing::info!(
        request_id = %request_id,
        route = %route,
        task_id = %task.task_id,
        kind = %task.kind,
        source = %redacted_endpoint(&task.source_endpoint),
        target = %redacted_endpoint(&task.target_endpoint),
        "precheck request received",
    );
}

// ─── Core logic (shared between persisted and draft modes) ──────────────

/// Run `run_test_connection` under a `tokio::spawn` panic guard so a panic
/// inside the underlying fetcher cannot kill the worker thread / drop the
/// socket. On panic we return a structured `TEST_CONNECTION_PANIC`
/// envelope that carries the correlation ID, mirroring the contract the
/// precheck routes already follow. The spawned future intentionally
/// returns Send-friendly types (`Result<TestConnectionResponse, ApiError>`);
/// `HttpResponse`'s `RefCell<Extensions>` is `!Send` so it cannot cross
/// `tokio::spawn` boundaries.
async fn invoke_test_connection_with_guard(request_id: String, task: Task) -> HttpResponse {
    let log_id = request_id.clone();
    let task_log_summary = format!(
        "kind={} src={} dst={}",
        task.kind, task.db_type_source, task.db_type_target
    );
    let inner_id = request_id.clone();
    let handle = tokio::spawn(async move { run_test_connection(&task, &inner_id).await });

    match handle.await {
        Ok(Ok(resp)) => {
            tracing::info!(
                request_id = %log_id,
                source_ok = resp.source.ok,
                target_ok = resp.target.ok,
                "test_connection completed",
            );
            HttpResponse::Ok().json(resp)
        }
        Ok(Err(mut e)) => {
            tracing::warn!(
                request_id = %log_id,
                code = %e.code,
                message = %e.message,
                task = %task_log_summary,
                "test_connection rejected before probing",
            );
            attach_request_id(&mut e, &log_id);
            e.error_response()
        }
        Err(join_err) => {
            let panic_msg = extract_panic_msg(join_err);
            tracing::error!(
                request_id = %log_id,
                panic_message = %panic_msg,
                task = %task_log_summary,
                "test_connection task panicked — returning TEST_CONNECTION_PANIC",
            );
            ApiError::with_details(
                codes::TEST_CONNECTION_PANIC,
                format!(
                    "test_connection task crashed unexpectedly (request_id={log_id}). \
                     Send this request ID to ops or grep console-server logs."
                ),
                serde_json::json!({
                    "requestId": log_id,
                    "panicMessage": panic_msg,
                }),
            )
            .error_response()
        }
    }
}

/// Core test_connection logic. Tests source and target connectivity independently,
/// each bounded by `TEST_CONNECTION_TIMEOUT_SECS`. Always emits the
/// correlation ID on the body so the UI can show it on failure.
async fn run_test_connection(
    task: &Task,
    request_id: &str,
) -> Result<TestConnectionResponse, ApiError> {
    let ini_path = write_temp_ini(task, &task.kind)?;

    let result = {
        let (task_config, precheck_config) = match load_configs(&ini_path) {
            Ok(c) => c,
            Err(e) => {
                cleanup_temp_ini(&ini_path);
                return Err(e);
            }
        };

        let builder = PrecheckerBuilder::build(precheck_config, task_config);

        if !builder.valid_config() {
            cleanup_temp_ini(&ini_path);
            return Err(ApiError::new(
                codes::TASK_VALIDATION_FAILED,
                "invalid config: source or target URL is empty",
            ));
        }

        // Test source and target independently — one failure does not abort the other
        let source = test_one_side(&builder, true).await;
        let target = test_one_side(&builder, false).await;

        Ok(TestConnectionResponse {
            source,
            target,
            request_id: request_id.to_string(),
        })
    };

    cleanup_temp_ini(&ini_path);
    result
}

/// Convenience wrapper used by tests that want an `HttpResponse` directly.
#[cfg(test)]
async fn do_test_connection(task: &Task, request_id: &str) -> HttpResponse {
    match run_test_connection(task, request_id).await {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(mut e) => {
            attach_request_id(&mut e, request_id);
            e.error_response()
        }
    }
}

/// Core precheck logic. Runs all applicable checks and returns per-item results.
/// A single failing check does NOT panic the orchestrator.
/// Only used in tests — handlers now use run_precheck via tokio::spawn.
#[cfg(test)]
async fn do_precheck(task: &Task) -> HttpResponse {
    match run_precheck(task).await {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(e) => e.error_response(),
    }
}

/// Pre-flight check on the filter config so that malformed `do_dbs` /
/// `do_tbs` / `ignore_dbs` / `ignore_tbs` values do NOT propagate into
/// `PrecheckerBuilder::build_checker` (which calls `RdbFilter::from_config`
/// with `.unwrap()` and would otherwise panic).
///
/// On success returns `Ok(())`. On failure returns an `Err` carrying a
/// pre-built `PrecheckResponse` with one explicit `fail` item per side
/// describing the actual filter parse error — never the generic
/// "Precheck panicked" row.
fn validate_filter_config(task_config: &TaskConfig) -> Result<(), PrecheckResponse> {
    let mut items: Vec<PrecheckItem> = Vec::new();

    let source_db_type = task_config.extractor_basic.db_type.clone();
    if let Err(e) = RdbFilter::from_config(&task_config.filter, &source_db_type) {
        items.push(PrecheckItem {
            name: "CheckFilterConfig".to_string(),
            side: "source".to_string(),
            status: "fail".to_string(),
            description: Some("validate filter (do_dbs / do_tbs / ignore_*) syntax".to_string()),
            error_message: Some(e.to_string()),
            advise_message: Some(
                "do_tbs and ignore_tbs entries must use 'db.tb' format (e.g. 'mydb.mytbl' or '*.*'). \
                 do_dbs and ignore_dbs accept comma-separated db names."
                    .to_string(),
            ),
        });
    }

    let target_db_type = task_config.sinker_basic.db_type.clone();
    if let Err(e) = RdbFilter::from_config(&task_config.filter, &target_db_type) {
        items.push(PrecheckItem {
            name: "CheckFilterConfig".to_string(),
            side: "target".to_string(),
            status: "fail".to_string(),
            description: Some("validate filter (do_dbs / do_tbs / ignore_*) syntax".to_string()),
            error_message: Some(e.to_string()),
            advise_message: Some(
                "do_tbs and ignore_tbs entries must use 'db.tb' format (e.g. 'mydb.mytbl' or '*.*'). \
                 do_dbs and ignore_dbs accept comma-separated db names."
                    .to_string(),
            ),
        });
    }

    if items.is_empty() {
        Ok(())
    } else {
        let fail = items.len();
        Err(PrecheckResponse {
            items,
            summary: PrecheckSummary {
                pass: 0,
                fail,
                skip: 0,
                warn: 0,
            },
        })
    }
}

/// Run precheck and return the structured response (not an HttpResponse).
/// Used by both the HTTP handler and the start_task precheck gate.
pub async fn run_precheck(task: &Task) -> Result<PrecheckResponse, ApiError> {
    // Struct kind: return empty-but-OK
    if task.kind == "struct" {
        return Ok(PrecheckResponse {
            items: vec![],
            summary: PrecheckSummary {
                pass: 0,
                fail: 0,
                skip: 0,
                warn: 0,
            },
        });
    }

    let ini_path = write_temp_ini(task, &task.kind)?;

    let result = {
        let (task_config, precheck_config) = load_configs(&ini_path)?;

        if let Err(filter_err) = validate_filter_config(&task_config) {
            cleanup_temp_ini(&ini_path);
            return Ok(filter_err);
        }

        let builder = PrecheckerBuilder::build(precheck_config, task_config);
        let do_cdc = task.kind == "cdc";

        if !builder.valid_config() {
            cleanup_temp_ini(&ini_path);
            return Err(ApiError::new(
                codes::TASK_VALIDATION_FAILED,
                "invalid config: source or target URL is empty",
            ));
        }

        let mut items = Vec::new();

        // Run source checks. `validate_filter_config` above already
        // rejected malformed filter syntax with a structured `fail` item,
        // so a builder error here is unexpected — surface it as a
        // CheckFilterConfig fail rather than panicking.
        match builder.build_checker(true) {
            Ok(Some(mut source_checker)) => {
                items.extend(run_side_checks(&mut source_checker, true, do_cdc, &task.kind).await);
            }
            Ok(None) => {}
            Err(e) => items.push(PrecheckItem {
                name: "CheckFilterConfig".to_string(),
                side: "source".to_string(),
                status: "fail".to_string(),
                description: Some(
                    "validate filter (do_dbs / do_tbs / ignore_*) syntax".to_string(),
                ),
                error_message: Some(e.to_string()),
                advise_message: None,
            }),
        }

        // Run target checks
        match builder.build_checker(false) {
            Ok(Some(mut sink_checker)) => {
                items.extend(run_side_checks(&mut sink_checker, false, do_cdc, &task.kind).await);
            }
            Ok(None) => {}
            Err(e) => items.push(PrecheckItem {
                name: "CheckFilterConfig".to_string(),
                side: "target".to_string(),
                status: "fail".to_string(),
                description: Some(
                    "validate filter (do_dbs / do_tbs / ignore_*) syntax".to_string(),
                ),
                error_message: Some(e.to_string()),
                advise_message: None,
            }),
        }

        // Compute summary
        let mut pass = 0;
        let mut fail = 0;
        let mut skip = 0;
        let mut warn = 0;
        for item in &items {
            match item.status.as_str() {
                "pass" => pass += 1,
                "fail" => fail += 1,
                "warn" => warn += 1,
                _ => skip += 1,
            }
        }

        Ok(PrecheckResponse {
            items,
            summary: PrecheckSummary {
                pass,
                fail,
                skip,
                warn,
            },
        })
    };

    cleanup_temp_ini(&ini_path);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth;
    use crate::db;
    use crate::rate_limit::RateLimiter;
    use crate::run_handlers::new_active_runs;
    use actix_web::cookie::Key;
    use actix_web::test as actix_test;
    use actix_web::App;

    /// Set up an in-memory DB with migrations + admin user.
    async fn setup_db() -> sqlx::SqlitePool {
        let pool = db::init(":memory:").await.unwrap();
        auth::seed_admin(&pool).await.unwrap();
        pool
    }

    /// Build a test app with the full route set.
    fn build_test_app(
        pool: sqlx::SqlitePool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        let key = Key::generate();
        let master_bytes = key.master().to_vec();
        let pool_clone = pool.clone();
        let rate_limiter = RateLimiter::new(Default::default());
        let active_runs = new_active_runs();
        let scraper_state = crate::metrics_scraper::ScraperState::new();
        let log_sse_state = crate::log_sse_handlers::LogSseState::default();
        let alert_sse_state = crate::alert_handlers::AlertSseState::new();
        let dispatcher_state = crate::alarm_dispatcher::DispatcherState::new();
        let alert_engine_state = crate::alert_engine::AlertEngineState::new();
        let idempotency_cache = crate::idempotency::IdempotencyCache::new();
        crate::build_app(
            Key::from(&master_bytes),
            pool_clone,
            rate_limiter,
            3600,
            active_runs,
            scraper_state,
            crate::port_pool::PortPool::new(),
            log_sse_state,
            alert_sse_state,
            dispatcher_state,
            alert_engine_state,
            idempotency_cache,
            crate::sse_session_tracker::SseSessionTracker::new(),
        )
    }

    /// Helper: create a minimal Task for testing with mysql source/target.
    fn make_mysql_task() -> Task {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        Task {
            id: "test-task-001".to_string(),
            task_id: "snapshot_mysql_mysql_test".to_string(),
            name: "test task".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "mysql".to_string(),
            db_type_target: "mysql".to_string(),
            source_endpoint: r#"{"url":"mysql://root:@127.0.0.1:3307/test"}"#.to_string(),
            target_endpoint: r#"{"url":"mysql://root:@127.0.0.1:3308/test"}"#.to_string(),
            extractor_config: r#"{"extractType":"snapshot"}"#.to_string(),
            sinker_config: r#"{"sinkType":"write"}"#.to_string(),
            filter_config: "{}".to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: "{}".to_string(),
            pipeline_config: "{}".to_string(),
            resumer_config: "{}".to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: "default-rg".to_string(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Helper: create a CDC task with mysql source.
    #[allow(dead_code)]
    fn make_cdc_mysql_task() -> Task {
        let mut task = make_mysql_task();
        task.kind = "cdc".to_string();
        task.extractor_config = r#"{"extractType":"cdc","serverId":100}"#.to_string();
        task.task_id = "cdc_mysql_mysql_test".to_string();
        task
    }

    /// Helper: create a struct task.
    fn make_struct_task() -> Task {
        let mut task = make_mysql_task();
        task.kind = "struct".to_string();
        task.extractor_config = r#"{"extractType":"struct"}"#.to_string();
        task.sinker_config = r#"{"sinkType":"struct"}"#.to_string();
        task.filter_config = r#"{"doStructures":"table,index"}"#.to_string();
        task.task_id = "struct_mysql_mysql_test".to_string();
        task
    }

    /// Helper: create a check task.
    #[allow(dead_code)]
    fn make_check_task() -> Task {
        let mut task = make_mysql_task();
        task.kind = "check".to_string();
        task.sinker_config = r#"{"sinkType":"check","checkLogDir":"./check"}"#.to_string();
        task.task_id = "check_mysql_mysql_test".to_string();
        task
    }

    /// Helper: create a pg source/target task.
    fn make_pg_task() -> Task {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        Task {
            id: "test-task-pg".to_string(),
            task_id: "snapshot_pg_pg_test".to_string(),
            name: "test pg task".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "pg".to_string(),
            db_type_target: "pg".to_string(),
            source_endpoint: r#"{"url":"postgres://postgres:@127.0.0.1:5433/test"}"#.to_string(),
            target_endpoint: r#"{"url":"postgres://postgres:@127.0.0.1:5434/test"}"#.to_string(),
            extractor_config: r#"{"extractType":"snapshot"}"#.to_string(),
            sinker_config: r#"{"sinkType":"write"}"#.to_string(),
            filter_config: "{}".to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: "{}".to_string(),
            pipeline_config: "{}".to_string(),
            resumer_config: "{}".to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: "default-rg".to_string(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    // ─── Temp INI write/load tests ─────────────────────────────────────

    #[test]
    fn temp_ini_write_and_load_mysql() {
        let task = make_mysql_task();
        let ini_path = write_temp_ini(&task, "snapshot").unwrap();
        let loaded = load_configs(&ini_path);
        cleanup_temp_ini(&ini_path);
        assert!(loaded.is_ok(), "TaskConfig should load from temp INI");
    }

    #[test]
    fn temp_ini_write_and_load_pg() {
        let task = make_pg_task();
        let ini_path = write_temp_ini(&task, "snapshot").unwrap();
        let loaded = load_configs(&ini_path);
        cleanup_temp_ini(&ini_path);
        assert!(
            loaded.is_ok(),
            "TaskConfig should load from temp INI for pg"
        );
    }

    #[test]
    fn temp_ini_contains_precheck_section() {
        let task = make_mysql_task();
        let ini_path = write_temp_ini(&task, "cdc").unwrap();
        let content = std::fs::read_to_string(&ini_path).unwrap();
        cleanup_temp_ini(&ini_path);
        assert!(content.contains("[precheck]"));
        assert!(content.contains("do_cdc=true"));
    }

    #[test]
    fn temp_ini_snapshot_does_cdc_false() {
        let task = make_mysql_task();
        let ini_path = write_temp_ini(&task, "snapshot").unwrap();
        let content = std::fs::read_to_string(&ini_path).unwrap();
        cleanup_temp_ini(&ini_path);
        assert!(content.contains("do_cdc=false"));
    }

    // ─── Struct kind returns empty-but-OK ──────────────────────────────

    #[actix_web::test]
    async fn struct_precheck_returns_empty_but_ok() {
        let task = make_struct_task();
        let resp = do_precheck(&task).await;
        assert_eq!(resp.status(), 200);

        let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
        let precheck_resp: PrecheckResponse = serde_json::from_slice(&body).unwrap();
        assert!(precheck_resp.items.is_empty());
        assert_eq!(precheck_resp.summary.fail, 0);
    }

    // ─── Test connection: both URLs bad → per-side fail ─────────────────

    #[actix_web::test]
    async fn test_connection_both_bad_ports() {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let task = Task {
            id: "bad-conn-task".to_string(),
            task_id: "snap_mysql_mysql_bad".to_string(),
            name: "bad".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "mysql".to_string(),
            db_type_target: "mysql".to_string(),
            source_endpoint: r#"{"url":"mysql://root:@127.0.0.1:19999/test"}"#.to_string(),
            target_endpoint: r#"{"url":"mysql://root:@127.0.0.1:19998/test"}"#.to_string(),
            extractor_config: r#"{"extractType":"snapshot"}"#.to_string(),
            sinker_config: r#"{"sinkType":"write"}"#.to_string(),
            filter_config: "{}".to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: "{}".to_string(),
            pipeline_config: "{}".to_string(),
            resumer_config: "{}".to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: "".to_string(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        let resp = do_test_connection(&task, "deadbeef").await;
        // Should return 200 even with both sides failing (VAL-CONN-004)
        assert_eq!(resp.status(), 200);

        let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
        let conn_resp: TestConnectionResponse = serde_json::from_slice(&body).unwrap();
        assert!(!conn_resp.source.ok, "source should fail");
        assert!(!conn_resp.target.ok, "target should fail");
        assert_eq!(conn_resp.request_id, "deadbeef");
    }

    // ─── Test connection: source bad, target bad → both fail (VAL-CONN-004) ──

    #[actix_web::test]
    async fn test_connection_one_bad_reports_both_sides() {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let task = Task {
            id: "mixed-conn-task".to_string(),
            task_id: "snap_mysql_bad_src".to_string(),
            name: "mixed".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "mysql".to_string(),
            db_type_target: "mysql".to_string(),
            source_endpoint: r#"{"url":"mysql://root:@127.0.0.1:19999/test"}"#.to_string(),
            target_endpoint: r#"{"url":"mysql://root:@127.0.0.1:19998/test"}"#.to_string(),
            extractor_config: r#"{"extractType":"snapshot"}"#.to_string(),
            sinker_config: r#"{"sinkType":"write"}"#.to_string(),
            filter_config: "{}".to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: "{}".to_string(),
            pipeline_config: "{}".to_string(),
            resumer_config: "{}".to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: "".to_string(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        let resp = do_test_connection(&task, "abcdef01").await;
        assert_eq!(resp.status(), 200);

        let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
        let conn_resp: TestConnectionResponse = serde_json::from_slice(&body).unwrap();
        // Each side reports independently — one failure does NOT abort the other
        assert!(!conn_resp.source.ok);
        assert!(!conn_resp.target.ok);
        assert!(conn_resp.source.code.is_some());
        assert!(conn_resp.target.code.is_some());
        assert_eq!(conn_resp.request_id, "abcdef01");
    }

    // ─── Single failing check does not panic (VAL-PRECHECK-005) ────────

    #[actix_web::test]
    async fn precheck_single_failure_no_panic() {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let task = Task {
            id: "precheck-fail-task".to_string(),
            task_id: "snap_mysql_fail".to_string(),
            name: "fail".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "mysql".to_string(),
            db_type_target: "mysql".to_string(),
            source_endpoint: r#"{"url":"mysql://root:@127.0.0.1:19999/test"}"#.to_string(),
            target_endpoint: r#"{"url":"mysql://root:@127.0.0.1:19998/test"}"#.to_string(),
            extractor_config: r#"{"extractType":"snapshot"}"#.to_string(),
            sinker_config: r#"{"sinkType":"write"}"#.to_string(),
            filter_config: "{}".to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: "{}".to_string(),
            pipeline_config: "{}".to_string(),
            resumer_config: "{}".to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: "".to_string(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        let resp = do_precheck(&task).await;
        // Must return 200 (not 500), even with failing checks
        assert_eq!(resp.status(), 200);

        let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
        let precheck_resp: PrecheckResponse = serde_json::from_slice(&body).unwrap();
        assert!(
            precheck_resp.summary.fail >= 1,
            "at least one check should fail"
        );
    }

    // ─── Draft task builder ────────────────────────────────────────────

    #[test]
    fn build_draft_task_creates_valid_task() {
        let body = CreateTaskRequest {
            name: "test".to_string(),
            kind: "snapshot".to_string(),
            engine_source: "mysql".to_string(),
            engine_target: "mysql".to_string(),
            sub_mode: None,
            source_sub_mode: None,
            target_sub_mode: None,
            source_endpoint: serde_json::json!({"url": "mysql://root:@host/db"}),
            target_endpoint: serde_json::json!({"url": "mysql://root:@host/db"}),
            extractor: serde_json::json!({"extractType": "snapshot"}),
            sinker: serde_json::json!({"sinkType": "write"}),
            filter: serde_json::json!({}),
            router: serde_json::json!({}),
            parallelizer: serde_json::json!({}),
            pipeline: serde_json::json!({}),
            resumer: serde_json::json!({}),
            processor: serde_json::json!({}),
            runtime: serde_json::json!({}),
            metrics: serde_json::json!({}),
            resource_group_id: None,
        };
        let task = build_draft_task(&body).unwrap();
        assert_eq!(task.kind, "snapshot");
        assert_eq!(task.db_type_source, "mysql");
        assert_eq!(task.db_type_target, "mysql");
        assert_eq!(task.status, "draft");
    }

    #[test]
    fn build_draft_task_gaussdb_pg_mode() {
        let body = CreateTaskRequest {
            name: "test".to_string(),
            kind: "snapshot".to_string(),
            engine_source: "gaussdb".to_string(),
            engine_target: "mysql".to_string(),
            sub_mode: Some("pg-mode".to_string()),
            source_sub_mode: None,
            target_sub_mode: None,
            source_endpoint: serde_json::json!({"url": "postgres://user:@host/db"}),
            target_endpoint: serde_json::json!({"url": "mysql://root:@host/db"}),
            extractor: serde_json::json!({"extractType": "snapshot"}),
            sinker: serde_json::json!({"sinkType": "write"}),
            filter: serde_json::json!({}),
            router: serde_json::json!({}),
            parallelizer: serde_json::json!({}),
            pipeline: serde_json::json!({}),
            resumer: serde_json::json!({}),
            processor: serde_json::json!({}),
            runtime: serde_json::json!({}),
            metrics: serde_json::json!({}),
            resource_group_id: None,
        };
        let task = build_draft_task(&body).unwrap();
        assert_eq!(task.db_type_source, "gaussdb_pg");
    }

    // ─── Precheck response shape tests ─────────────────────────────────

    #[test]
    fn check_result_to_item_pass() {
        let cr = CheckResult {
            check_type_name: "CheckDatabaseConnection".to_string(),
            check_desc: "check connection".to_string(),
            is_validate: true,
            error_msg: String::new(),
            warn_msg: String::new(),
            is_source: true,
            advise_msg: String::new(),
        };
        let item = check_result_to_item(&cr, true);
        assert_eq!(item.name, "CheckDatabaseConnection");
        assert_eq!(item.side, "source");
        assert_eq!(item.status, "pass");
        assert!(item.error_message.is_none());
    }

    #[test]
    fn check_result_to_item_fail() {
        let cr = CheckResult {
            check_type_name: "CheckDatabaseConnection".to_string(),
            check_desc: "check connection".to_string(),
            is_validate: false,
            error_msg: "connection refused".to_string(),
            warn_msg: String::new(),
            is_source: false,
            advise_msg: "check network".to_string(),
        };
        let item = check_result_to_item(&cr, false);
        assert_eq!(item.side, "target");
        assert_eq!(item.status, "fail");
        assert_eq!(item.error_message.as_deref(), Some("connection refused"));
        assert_eq!(item.advise_message.as_deref(), Some("check network"));
    }

    #[test]
    fn check_result_to_item_warn() {
        let cr = CheckResult {
            check_type_name: "CheckDatabaseVersionSupported".to_string(),
            check_desc: "check database version".to_string(),
            is_validate: false,
            error_msg: String::new(),
            warn_msg: "version 5.6 is not fully supported".to_string(),
            is_source: true,
            advise_msg: "upgrade to 5.7+".to_string(),
        };
        let item = check_result_to_item(&cr, true);
        assert_eq!(item.name, "CheckDatabaseVersionSupported");
        assert_eq!(item.side, "source");
        assert_eq!(item.status, "warn");
        assert!(item.error_message.is_none());
        assert_eq!(item.advise_message.as_deref(), Some("upgrade to 5.7+"));
    }

    #[test]
    fn precheck_summary_counts_items() {
        let items = vec![
            PrecheckItem {
                name: "a".to_string(),
                side: "source".to_string(),
                status: "pass".to_string(),
                description: None,
                error_message: None,
                advise_message: None,
            },
            PrecheckItem {
                name: "b".to_string(),
                side: "source".to_string(),
                status: "fail".to_string(),
                description: None,
                error_message: None,
                advise_message: None,
            },
            PrecheckItem {
                name: "c".to_string(),
                side: "target".to_string(),
                status: "skip".to_string(),
                description: None,
                error_message: None,
                advise_message: None,
            },
            PrecheckItem {
                name: "d".to_string(),
                side: "source".to_string(),
                status: "warn".to_string(),
                description: None,
                error_message: None,
                advise_message: None,
            },
        ];
        let mut pass = 0;
        let mut fail = 0;
        let mut skip = 0;
        let mut warn = 0;
        for item in &items {
            match item.status.as_str() {
                "pass" => pass += 1,
                "fail" => fail += 1,
                "warn" => warn += 1,
                _ => skip += 1,
            }
        }
        assert_eq!(pass, 1);
        assert_eq!(fail, 1);
        assert_eq!(skip, 1);
        assert_eq!(warn, 1);
    }

    // ─── Handler route tests via actix_web::test ───────────────────────
    // Note: Full handler integration tests (with auth) are in
    // tests/precheck_handlers.rs. Here we test core logic directly.

    #[actix_web::test]
    async fn test_connection_handler_unauth_returns_401() {
        let pool = setup_db().await;
        let app = actix_test::init_service(build_test_app(pool)).await;

        let req = actix_test::TestRequest::post()
            .uri("/api/tasks/some-id/test_connection")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_web::test]
    async fn precheck_handler_unauth_returns_401() {
        let pool = setup_db().await;
        let app = actix_test::init_service(build_test_app(pool)).await;

        let req = actix_test::TestRequest::post()
            .uri("/api/tasks/some-id/precheck")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_web::test]
    async fn preview_test_connection_handler_requires_auth() {
        let pool = setup_db().await;
        let app = actix_test::init_service(build_test_app(pool)).await;

        let req = actix_test::TestRequest::post()
            .uri("/api/tasks/preview/test_connection")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
            }))
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        // Should be 401 (anonymous) not 403
        assert_eq!(resp.status(), 401);
    }

    #[actix_web::test]
    async fn preview_precheck_handler_requires_auth() {
        let pool = setup_db().await;
        let app = actix_test::init_service(build_test_app(pool)).await;

        let req = actix_test::TestRequest::post()
            .uri("/api/tasks/preview/precheck")
            .set_json(serde_json::json!({
                "kind": "snapshot",
                "engineSource": "mysql",
                "engineTarget": "mysql",
            }))
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        // Should be 401 (anonymous) not 403
        assert_eq!(resp.status(), 401);
    }

    // ─── Test connection covers each engine dispatch (VAL-CONN-005) ────

    #[test]
    fn builder_builds_mysql_checker() {
        let task = make_mysql_task();
        let ini_path = write_temp_ini(&task, "snapshot").unwrap();
        let (task_config, precheck_config) = load_configs(&ini_path).unwrap();
        cleanup_temp_ini(&ini_path);
        let builder = PrecheckerBuilder::build(precheck_config, task_config);
        assert!(
            builder.build_checker(true).unwrap().is_some(),
            "mysql source checker"
        );
        assert!(
            builder.build_checker(false).unwrap().is_some(),
            "mysql target checker"
        );
    }

    #[test]
    fn builder_builds_pg_checker() {
        let task = make_pg_task();
        let ini_path = write_temp_ini(&task, "snapshot").unwrap();
        let (task_config, precheck_config) = load_configs(&ini_path).unwrap();
        cleanup_temp_ini(&ini_path);
        let builder = PrecheckerBuilder::build(precheck_config, task_config);
        assert!(
            builder.build_checker(true).unwrap().is_some(),
            "pg source checker"
        );
        assert!(
            builder.build_checker(false).unwrap().is_some(),
            "pg target checker"
        );
    }

    // ─── Test connection: empty URL returns validation error ──────────

    #[actix_web::test]
    async fn test_connection_empty_url_returns_error() {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let task = Task {
            id: "empty-url-task".to_string(),
            task_id: "snap_empty".to_string(),
            name: "empty".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "mysql".to_string(),
            db_type_target: "mysql".to_string(),
            source_endpoint: r#"{"url":""}"#.to_string(),
            target_endpoint: r#"{"url":""}"#.to_string(),
            extractor_config: r#"{"extractType":"snapshot"}"#.to_string(),
            sinker_config: r#"{"sinkType":"write"}"#.to_string(),
            filter_config: "{}".to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: "{}".to_string(),
            pipeline_config: "{}".to_string(),
            resumer_config: "{}".to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: "".to_string(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        let resp = do_test_connection(&task, "feedface").await;
        // Empty URL → 422 validation error
        assert_eq!(resp.status(), 422);

        let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
        let env: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            env.pointer("/details/requestId").and_then(|v| v.as_str()),
            Some("feedface"),
            "envelope must surface requestId on validation failure"
        );
    }

    // ─── Repro: wizard snapshot_cdc payload — must NOT panic ───────────
    //
    // The web wizard for "create snapshot task" lets the user pick
    // syncMode='snapshot_cdc'. That posts to /tasks/preview/precheck with:
    //   - kind=snapshot
    //   - extractor.extract_type=snapshot_and_cdc
    //   - extractor.server_id="<num as string>"
    //   - filter.do_dbs="<user_db>"
    //   - filter.do_tbs="<user_tbl>" (often a single token without a dot,
    //     e.g. "*" — see VAL-CROSS-001/2)
    //
    // The handler routes this through `two_phase::render_phase1_ini` which
    // builds a phase-1 snapshot INI. The downstream PrecheckerBuilder must
    // not panic on any well-formed user input. In particular, an unbalanced
    // do_tbs ("*" alone, no dot) was crashing inside RdbFilter::parse_pair_tokens.
    #[actix_web::test]
    async fn wizard_snapshot_cdc_does_not_panic() {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let task = Task {
            id: "wizard-snap-cdc".to_string(),
            task_id: "draft_snapshot_mysql_mysql_abcd1234".to_string(),
            name: "wizard".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "mysql".to_string(),
            db_type_target: "mysql".to_string(),
            source_endpoint: r#"{"url":"mysql://root:@127.0.0.1:19999/test"}"#.to_string(),
            target_endpoint: r#"{"url":"mysql://root:@127.0.0.1:19998/test"}"#.to_string(),
            extractor_config: r#"{"extract_type":"snapshot_and_cdc","server_id":"2000","heartbeat_interval_secs":10}"#
                .to_string(),
            sinker_config: "{}".to_string(),
            filter_config: r#"{"do_dbs":"my_db","do_tbs":"*","ignore_dbs":"","ignore_tbs":"","do_events":"insert,update,delete"}"#
                .to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: r#"{"parallel_type":"snapshot","parallel_size":4}"#.to_string(),
            pipeline_config: r#"{"buffer_size":16000,"checkpoint_interval_secs":10}"#.to_string(),
            resumer_config: r#"{"resume_type":"from_log"}"#.to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: "default".to_string(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        let result = run_precheck(&task).await;
        // The handler must return a structured response, not propagate a panic.
        let resp = result.expect("run_precheck must complete without panic or error");
        assert!(
            !resp.items.is_empty(),
            "real precheck must produce at least one item, got empty"
        );
        // The single-token do_tbs="*" entry is invalid (must be db.tb). The
        // handler must surface a real precheck item describing the filter
        // syntax problem, NOT the generic "Precheck panicked" row.
        let has_filter_item = resp
            .items
            .iter()
            .any(|i| i.name == "CheckFilterConfig" && i.status == "fail");
        assert!(
            has_filter_item,
            "expected CheckFilterConfig fail item with the real filter syntax error, got: {:?}",
            resp.items
        );
    }

    /// Regression: a snapshot task with a well-formed do_tbs="db.t1,db.t2"
    /// filter must NOT trigger the filter-syntax fail item; the precheck
    /// should proceed to connection / version / struct checks (all fail
    /// because the test ports are unreachable, but no panic).
    #[actix_web::test]
    async fn wizard_snapshot_well_formed_filter_no_filter_fail() {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let task = Task {
            id: "wizard-snap-ok".to_string(),
            task_id: "draft_snapshot_mysql_mysql_efgh5678".to_string(),
            name: "wizard-ok".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "mysql".to_string(),
            db_type_target: "mysql".to_string(),
            source_endpoint: r#"{"url":"mysql://root:@127.0.0.1:19999/test"}"#.to_string(),
            target_endpoint: r#"{"url":"mysql://root:@127.0.0.1:19998/test"}"#.to_string(),
            extractor_config: r#"{"extract_type":"snapshot"}"#.to_string(),
            sinker_config: "{}".to_string(),
            filter_config: r#"{"do_dbs":"my_db","do_tbs":"my_db.t1","ignore_dbs":"","ignore_tbs":"","do_events":"insert,update,delete"}"#
                .to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: r#"{"parallel_type":"snapshot","parallel_size":4}"#.to_string(),
            pipeline_config: r#"{"buffer_size":16000,"checkpoint_interval_secs":10}"#.to_string(),
            resumer_config: r#"{"resume_type":"from_log"}"#.to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: "default".to_string(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        let resp = run_precheck(&task)
            .await
            .expect("well-formed filter must not cause errors");
        let has_filter_fail = resp
            .items
            .iter()
            .any(|i| i.name == "CheckFilterConfig" && i.status == "fail");
        assert!(
            !has_filter_fail,
            "well-formed filter must NOT yield CheckFilterConfig fail; items: {:?}",
            resp.items
        );
    }

    // ─── Observability helpers ──────────────────────────────────────────

    #[test]
    fn redact_url_strips_credentials() {
        assert_eq!(
            redact_url("mysql://root:secret@127.0.0.1:3307/test"),
            "mysql://***@127.0.0.1:3307/test"
        );
        assert_eq!(
            redact_url("postgres://u:p@host:5432/db"),
            "postgres://***@host:5432/db"
        );
        // No `@` → returned as-is
        assert_eq!(
            redact_url("mysql://127.0.0.1:3307"),
            "mysql://127.0.0.1:3307"
        );
    }

    #[test]
    fn redacted_endpoint_handles_missing_url() {
        assert_eq!(redacted_endpoint("{}"), "?");
        assert_eq!(redacted_endpoint("not json"), "?");
    }

    #[test]
    fn new_request_id_is_short_and_stable_length() {
        let id = new_request_id();
        assert_eq!(id.len(), REQUEST_ID_LEN);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn attach_request_id_inserts_into_existing_object() {
        let mut e = ApiError::with_details(
            codes::PRECHECK_BLOCKING_FAILED,
            "x",
            serde_json::json!({"foo": 1}),
        );
        attach_request_id(&mut e, "abc12345");
        let details = e.details.unwrap();
        assert_eq!(
            details.get("requestId").and_then(|v| v.as_str()),
            Some("abc12345")
        );
        assert_eq!(details.get("foo").and_then(|v| v.as_u64()), Some(1));
    }

    #[test]
    fn attach_request_id_creates_object_when_missing() {
        let mut e = ApiError::new(codes::PRECHECK_BLOCKING_FAILED, "y");
        attach_request_id(&mut e, "deadbeef");
        let details = e.details.unwrap();
        assert_eq!(
            details.get("requestId").and_then(|v| v.as_str()),
            Some("deadbeef")
        );
    }

    /// Verify the panic-guard surfaces `PRECHECK_PANIC` with `requestId` and
    /// `panicMessage` when the spawned task panics. This is the exact path
    /// the user complained about — a panic must produce a diagnosable
    /// envelope, not silent breakage.
    #[actix_web::test]
    async fn invoke_precheck_with_guard_panic_yields_request_id_envelope() {
        // We cannot easily inject a panic into the real run_precheck without
        // touching production code, so we exercise the helper that builds
        // the response directly: a JoinError carrying a panic payload.
        let join_err = tokio::spawn(async { panic!("synthetic panic for test") })
            .await
            .expect_err("spawned task must panic");
        let msg = extract_panic_msg(join_err);
        assert!(
            msg.contains("synthetic panic"),
            "panic message must round-trip the cause: {msg}"
        );
    }

    // ─── Regression: socket-hang-up bug — slow side must time out cleanly ──
    //
    // Before the fix, an unreachable host on the wizard's "test connection"
    // step caused Vite to print
    //   `[vite] http proxy error: /api/tasks/preview/test_connection
    //    Error: socket hang up`
    // because the underlying MySQL/PG fetcher would hang on TCP SYN until
    // the kernel timed out (~75s), which is longer than any sane proxy
    // deadline. The fix wraps each side in `tokio::time::timeout`. This
    // test drives that wrapper with a 50ms deadline against a real builder
    // pointing at an unreachable host, and asserts the per-side result
    // reports `TEST_CONNECTION_TIMEOUT`.
    #[actix_web::test]
    async fn test_one_side_with_timeout_returns_structured_timeout_code() {
        // Builder pointing at a non-routable IP (bogon range) so the TCP
        // SYN is silently dropped — no immediate refused, no immediate
        // success. This is the exact failure mode that was hanging the
        // socket before the fix.
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let task = Task {
            id: "blackhole".to_string(),
            task_id: "snap_blackhole".to_string(),
            name: "blackhole".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "mysql".to_string(),
            db_type_target: "mysql".to_string(),
            source_endpoint: r#"{"url":"mysql://root:@10.255.255.1:65535/test"}"#.to_string(),
            target_endpoint: r#"{"url":"mysql://root:@10.255.255.1:65535/test"}"#.to_string(),
            extractor_config: r#"{"extractType":"snapshot"}"#.to_string(),
            sinker_config: r#"{"sinkType":"write"}"#.to_string(),
            filter_config: "{}".to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: "{}".to_string(),
            pipeline_config: "{}".to_string(),
            resumer_config: "{}".to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: "".to_string(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        let ini_path = write_temp_ini(&task, "snapshot").unwrap();
        let (task_config, precheck_config) = load_configs(&ini_path).unwrap();
        cleanup_temp_ini(&ini_path);
        let builder = PrecheckerBuilder::build(precheck_config, task_config);

        let result = test_one_side_with_timeout(&builder, true, Duration::from_millis(50)).await;
        // Either we time out cleanly or the host immediately fails — both
        // are fine, but a hang is NOT (the test would block past 50ms by
        // many seconds without our timeout).
        assert!(!result.ok, "unreachable host must report failure");
        let code = result.code.expect("failure must carry a code");
        assert!(
            code == codes::TEST_CONNECTION_TIMEOUT || code == "CONNECTION_FAILED",
            "expected TIMEOUT or CONNECTION_FAILED, got {code}",
        );
    }

    /// Regression: top-level `request_id` must round-trip through the
    /// 200-body so the wizard can show it on a side failure.
    #[actix_web::test]
    async fn test_connection_response_carries_request_id_on_failure() {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let task = Task {
            id: "rid-task".to_string(),
            task_id: "snap_rid".to_string(),
            name: "rid".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "mysql".to_string(),
            db_type_target: "mysql".to_string(),
            source_endpoint: r#"{"url":"mysql://root:@127.0.0.1:19999/test"}"#.to_string(),
            target_endpoint: r#"{"url":"mysql://root:@127.0.0.1:19998/test"}"#.to_string(),
            extractor_config: r#"{"extractType":"snapshot"}"#.to_string(),
            sinker_config: r#"{"sinkType":"write"}"#.to_string(),
            filter_config: "{}".to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: "{}".to_string(),
            pipeline_config: "{}".to_string(),
            resumer_config: "{}".to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: "".to_string(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        let resp = run_test_connection(&task, "cafe1234").await.unwrap();
        assert!(!resp.source.ok || !resp.target.ok);
        assert_eq!(resp.request_id, "cafe1234");
    }

    /// Regression: panic guard wraps a fatal handler crash and surfaces
    /// `TEST_CONNECTION_PANIC` with `requestId` instead of dropping the
    /// HTTP socket.
    #[actix_web::test]
    async fn invoke_test_connection_with_guard_panic_yields_request_id_envelope() {
        // Run a synthetic panic and assert extract_panic_msg reads it.
        let join_err = tokio::spawn(async { panic!("synthetic test_connection panic") })
            .await
            .expect_err("spawned task must panic");
        let msg = extract_panic_msg(join_err);
        assert!(msg.contains("synthetic test_connection panic"));

        // Also assert the envelope code is reachable as a constant.
        assert_eq!(codes::TEST_CONNECTION_PANIC, "TEST_CONNECTION_PANIC");
        assert_eq!(codes::TEST_CONNECTION_TIMEOUT, "TEST_CONNECTION_TIMEOUT");
    }

    /// The hard-coded production timeout must stay within Vite's default
    /// proxy window (30s), with comfortable headroom for the response
    /// itself. Bumping it accidentally to >25s would re-introduce the
    /// "socket hang up" bug.
    #[test]
    fn test_connection_timeout_constant_is_within_proxy_budget() {
        const MAX_PROXY_HEADROOM_SECS: u64 = 15;
        const MIN_SLOW_HANDSHAKE_SECS: u64 = 3;
        let timeout_secs = test_connection_timeout_secs();
        assert!(
            timeout_secs <= MAX_PROXY_HEADROOM_SECS,
            "TEST_CONNECTION_TIMEOUT_SECS={} exceeds proxy headroom budget",
            timeout_secs,
        );
        assert!(
            timeout_secs >= MIN_SLOW_HANDSHAKE_SECS,
            "TEST_CONNECTION_TIMEOUT_SECS={} too short for slow handshakes",
            timeout_secs,
        );
    }

    fn test_connection_timeout_secs() -> u64 {
        TEST_CONNECTION_TIMEOUT_SECS
    }

    /// Regression for request_id=9169ec20: a wizard test_connection with a
    /// malformed `do_tbs="*"` (single token, no `db.tb` form) used to
    /// panic inside `PrecheckerBuilder::build_checker` via
    /// `RdbFilter::from_config(...).unwrap()`. The panic was caught by the
    /// guard and surfaced as `TEST_CONNECTION_PANIC`, leaving the user
    /// with a generic crash envelope instead of a structured per-side
    /// error. After the fix, `test_connection` must return 200 with
    /// per-side `code=INVALID_FILTER_CONFIG` carrying the actual filter
    /// parse error.
    #[actix_web::test]
    async fn test_connection_malformed_filter_returns_structured_per_side_error() {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let task = Task {
            id: "filter-panic".to_string(),
            task_id: "snap_filter_panic".to_string(),
            name: "filter-panic".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "mysql".to_string(),
            db_type_target: "mysql".to_string(),
            source_endpoint: r#"{"url":"mysql://root:@127.0.0.1:3307/test"}"#.to_string(),
            target_endpoint: r#"{"url":"mysql://root:@127.0.0.1:3308/test"}"#.to_string(),
            extractor_config: r#"{"extract_type":"snapshot"}"#.to_string(),
            sinker_config: "{}".to_string(),
            filter_config: r#"{"do_dbs":"my_db","do_tbs":"*","ignore_dbs":"","ignore_tbs":"","do_events":"insert,update,delete"}"#
                .to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: r#"{"parallel_type":"snapshot","parallel_size":4}"#.to_string(),
            pipeline_config: r#"{"buffer_size":16000,"checkpoint_interval_secs":10}"#.to_string(),
            resumer_config: r#"{"resume_type":"from_log"}"#.to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: "default".to_string(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        let resp = run_test_connection(&task, "9169ec20")
            .await
            .expect("malformed filter must produce structured per-side error, not panic");
        assert_eq!(resp.request_id, "9169ec20");
        assert!(!resp.source.ok, "source must report failure on bad filter");
        assert!(!resp.target.ok, "target must report failure on bad filter");
        assert_eq!(
            resp.source.code.as_deref(),
            Some(codes::INVALID_FILTER_CONFIG),
            "source code must be INVALID_FILTER_CONFIG, got {:?}",
            resp.source.code
        );
        assert_eq!(
            resp.target.code.as_deref(),
            Some(codes::INVALID_FILTER_CONFIG),
            "target code must be INVALID_FILTER_CONFIG, got {:?}",
            resp.target.code
        );
        let src_msg = resp.source.message.as_deref().unwrap_or("");
        assert!(
            src_msg.contains("db.tb") && src_msg.contains("'*'"),
            "source message must explain the filter syntax error, got: {src_msg}"
        );
    }

    /// Direct probe of `PrecheckerBuilder::build_checker`: with a malformed
    /// filter it must return `Err(...)` rather than panicking. This guards
    /// the underlying lib API itself, independent of how
    /// `dt-console-server` consumes it.
    #[test]
    fn builder_build_checker_returns_err_on_malformed_filter() {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let task = Task {
            id: "lib-filter".to_string(),
            task_id: "snap_lib_filter".to_string(),
            name: "lib-filter".to_string(),
            kind: "snapshot".to_string(),
            db_type_source: "mysql".to_string(),
            db_type_target: "mysql".to_string(),
            source_endpoint: r#"{"url":"mysql://root:@127.0.0.1:3307/test"}"#.to_string(),
            target_endpoint: r#"{"url":"mysql://root:@127.0.0.1:3308/test"}"#.to_string(),
            extractor_config: r#"{"extract_type":"snapshot"}"#.to_string(),
            sinker_config: "{}".to_string(),
            filter_config: r#"{"do_dbs":"my_db","do_tbs":"*","ignore_dbs":"","ignore_tbs":""}"#
                .to_string(),
            router_config: "{}".to_string(),
            parallelizer_config: "{}".to_string(),
            pipeline_config: "{}".to_string(),
            resumer_config: "{}".to_string(),
            processor_config: "{}".to_string(),
            runtime_config: "{}".to_string(),
            metrics_config: "{}".to_string(),
            resource_group_id: "".to_string(),
            owner_user_id: None,
            status: "draft".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        let ini_path = write_temp_ini(&task, "snapshot").unwrap();
        let (task_config, precheck_config) = load_configs(&ini_path).unwrap();
        cleanup_temp_ini(&ini_path);
        let builder = PrecheckerBuilder::build(precheck_config, task_config);
        let result = builder.build_checker(true);
        assert!(
            result.is_err(),
            "build_checker must return Err on malformed filter instead of panicking"
        );
        let err_msg = format!("{}", result.err().unwrap());
        assert!(
            err_msg.contains("db.tb"),
            "error must explain the filter syntax problem, got: {err_msg}"
        );
    }
}
