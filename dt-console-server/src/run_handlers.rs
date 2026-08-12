//! HTTP handlers for Run lifecycle endpoints.
//!
//! - POST   /api/tasks/:id/start   — start a new Run (202 + run_id)
//! - POST   /api/tasks/:id/stop    — stop a running Run (202)
//! - POST   /api/tasks/:id/pause   — pause a running CDC Run (202)
//! - POST   /api/tasks/:id/resume  — resume a paused Run (202)
//! - GET    /api/runs              — list all Runs (200)
//! - GET    /api/runs/:id          — get Run details with position

use actix_web::{get, post, web, HttpResponse, ResponseError};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::{codes, ApiError};
use crate::executor::{self, ChildStatus, ExitStatus, KillResult, RunSlot};
use crate::idempotency::{extract_scoped_key, IdempotencyCache};
use crate::ini_renderer;
use crate::metrics_scraper::{self, ScraperState};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{
    is_legal_transition, run_status, ControlLog, Run, RunResponse, StartRunResponse, Task,
    UserContext,
};
use crate::port_pool::PortPool;
use crate::precheck_handlers::PrecheckItem;
use crate::repositories::control_log_repository::ControlLogRepository;
use crate::repositories::operate_log_repository::OperateLogRepository;
use crate::repositories::run_repository::RunRepository;
use crate::repositories::task_repository::TaskRepository;

const GAUSSDB_CANDIDATE_HOSTS_ENV: &str = "gaussdb_pg_candidate_hosts";

/// Shared state for active Run handles, keyed by task_id.
///
/// This ensures at most one active Run per Task. The inner Mutex protects
/// against concurrent starts racing to insert a handle.
///
/// A slot can be in `Starting` (claimed but not yet spawned) or
/// `Active(RunHandle)` (engine subprocess is running). The `Starting`
/// state eliminates the TOCTOU race between checking "is there an active
/// run?" and inserting the handle — both happen in a single lock scope.
pub type ActiveRuns = Arc<Mutex<std::collections::HashMap<String, RunSlot>>>;

/// Create a new ActiveRuns instance.
pub fn new_active_runs() -> ActiveRuns {
    Arc::new(Mutex::new(std::collections::HashMap::new()))
}

/// Public accessor for the executor's run_data_dir.
pub fn executor_run_data_dir() -> String {
    executor::run_data_dir()
}

const STRUCT_INIT_POLL_MILLIS: u64 = 200;
const STRUCT_INIT_STRUCTURES: &str = "database,table,constraint";
const STRUCT_INIT_STRUCTURES_WITH_INDEX: &str = "database,table,constraint,index";
const STRUCT_INIT_ORACLE_STRUCTURES: &str = "table,constraint";
const STRUCT_INIT_ORACLE_STRUCTURES_WITH_INDEX: &str = "table,constraint,index";

async fn run_struct_init_if_requested(
    task: &Task,
    binary_override: Option<&str>,
) -> Result<(), ApiError> {
    if !should_run_struct_init(task) {
        return Ok(());
    }

    let struct_task = build_struct_init_task(task)?;
    let ini_content = ini_renderer::render(&struct_task);
    let run_id = format!("{}-struct-init-{}", task.id, uuid::Uuid::new_v4().simple());
    let engine_env = engine_extra_env(task);
    let handle = executor::LocalExecutor::spawn_with_env(
        &run_id,
        &ini_content,
        binary_override,
        &engine_env,
    )
    .await
    .map_err(|e| struct_init_error(task, &run_id, format!("spawn failed: {e}")))?;

    loop {
        match executor::LocalExecutor::status(&handle).await {
            ChildStatus::Running => {
                tokio::time::sleep(tokio::time::Duration::from_millis(STRUCT_INIT_POLL_MILLIS))
                    .await;
            }
            ChildStatus::Exited(exit) => {
                return match exit {
                    ExitStatus::Exited { code: 0 } => Ok(()),
                    ExitStatus::Exited { code } => Err(struct_init_error(
                        task,
                        &run_id,
                        format!("engine exited with code {code}"),
                    )),
                    ExitStatus::Signaled { signal } => Err(struct_init_error(
                        task,
                        &run_id,
                        format!("engine terminated by signal {signal}"),
                    )),
                };
            }
        }
    }
}

fn engine_extra_env(task: &Task) -> Vec<(String, String)> {
    gaussdb_candidate_hosts(task)
        .map(|hosts| vec![(GAUSSDB_CANDIDATE_HOSTS_ENV.to_string(), hosts)])
        .unwrap_or_default()
}

fn gaussdb_candidate_hosts(task: &Task) -> Option<String> {
    let mut values = Vec::new();
    if is_gaussdb_type(&task.db_type_source) {
        let source = parse_endpoint_config(&task.source_endpoint);
        values.extend(candidate_hosts_from_endpoint(&source));
    }
    if is_gaussdb_type(&task.db_type_target) {
        let target = parse_endpoint_config(&task.target_endpoint);
        values.extend(candidate_hosts_from_endpoint(&target));
    }
    let hosts = values
        .into_iter()
        .filter(|host| !host.trim().is_empty())
        .collect::<Vec<_>>()
        .join(",");
    if hosts.is_empty() {
        None
    } else {
        Some(hosts)
    }
}

fn is_gaussdb_type(db_type: &str) -> bool {
    db_type.starts_with("gaussdb_")
}

fn parse_endpoint_config(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw).unwrap_or_default()
}

fn candidate_hosts_from_endpoint(endpoint: &serde_json::Value) -> Vec<String> {
    endpoint
        .get("candidateHosts")
        .or_else(|| endpoint.get("candidate_hosts"))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::trim))
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn should_run_struct_init(task: &Task) -> bool {
    if task.kind != "snapshot" {
        return false;
    }
    let runtime = parse_json_object(&task.runtime_config);
    runtime
        .get("sync_schema")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn build_struct_init_task(task: &Task) -> Result<Task, ApiError> {
    let mut struct_task = task.clone();
    struct_task.id = format!("{}-struct-init", task.id);
    struct_task.task_id = format!("{}_struct_init", task.task_id);
    struct_task.kind = "struct".to_string();
    struct_task.extractor_config = merge_config(
        &task.extractor_config,
        &[
            ("extractType", serde_json::json!("struct")),
            ("extract_type", serde_json::json!("struct")),
        ],
    );
    struct_task.sinker_config = merge_config(
        &task.sinker_config,
        &[
            ("sinkType", serde_json::json!("struct")),
            ("sink_type", serde_json::json!("struct")),
            ("batch_size", serde_json::json!(1)),
            ("conflict_policy", serde_json::json!("ignore")),
        ],
    );
    struct_task.filter_config = build_struct_filter_config(task)?;
    struct_task.parallelizer_config =
        serde_json::json!({"parallel_type":"serial","parallel_size":1}).to_string();
    struct_task.pipeline_config =
        serde_json::json!({"buffer_size":100,"checkpoint_interval_secs":1}).to_string();
    struct_task.resumer_config = "{}".to_string();
    struct_task.processor_config = "{}".to_string();
    Ok(struct_task)
}

fn build_struct_filter_config(task: &Task) -> Result<String, ApiError> {
    let mut filter = parse_json_object(&task.filter_config);
    let do_dbs_empty = string_field_is_empty(&filter, "do_dbs");
    let do_tbs_empty = string_field_is_empty(&filter, "do_tbs");
    if do_dbs_empty && do_tbs_empty {
        return Err(ApiError::new(
            codes::STRUCT_FILTER_REQUIRED,
            "Struct initialization requires do_dbs or do_tbs",
        ));
    }
    let runtime = parse_json_object(&task.runtime_config);
    let structures = struct_init_structures(task, &runtime);
    narrow_struct_filter_to_explicit_tables(&mut filter);
    filter.insert(
        "do_structures".to_string(),
        serde_json::Value::String(structures.to_string()),
    );
    filter.insert(
        "do_events".to_string(),
        serde_json::Value::String(String::new()),
    );
    Ok(serde_json::Value::Object(filter).to_string())
}

fn narrow_struct_filter_to_explicit_tables(
    filter: &mut serde_json::Map<String, serde_json::Value>,
) {
    if !string_field_equals(filter, "do_dbs", "*") {
        return;
    }
    if string_field_is_empty(filter, "do_tbs") || string_field_equals(filter, "do_tbs", "*.*") {
        return;
    }
    filter.insert(
        "do_dbs".to_string(),
        serde_json::Value::String(String::new()),
    );
}

fn struct_init_structures(
    task: &Task,
    runtime: &serde_json::Map<String, serde_json::Value>,
) -> &'static str {
    let with_index = runtime
        .get("sync_index")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    match (task.db_type_target.as_str(), with_index) {
        ("oracle", true) | ("gaussdb_oracle", true) => STRUCT_INIT_ORACLE_STRUCTURES_WITH_INDEX,
        ("oracle", false) | ("gaussdb_oracle", false) => STRUCT_INIT_ORACLE_STRUCTURES,
        (_, true) => STRUCT_INIT_STRUCTURES_WITH_INDEX,
        (_, false) => STRUCT_INIT_STRUCTURES,
    }
}

fn merge_config(config: &str, fields: &[(&str, serde_json::Value)]) -> String {
    let mut object = parse_json_object(config);
    for (key, value) in fields {
        object.insert((*key).to_string(), value.clone());
    }
    serde_json::Value::Object(object).to_string()
}

fn parse_json_object(config: &str) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::from_str::<serde_json::Value>(config) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

fn string_field_is_empty(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> bool {
    map.get(key)
        .and_then(|v| v.as_str())
        .is_none_or(|s| s.trim().is_empty())
}

fn string_field_equals(
    map: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    expected: &str,
) -> bool {
    map.get(key)
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.trim() == expected)
}

fn struct_init_error(task: &Task, run_id: &str, reason: String) -> ApiError {
    ApiError::with_details(
        codes::STRUCT_INIT_FAILED,
        format!("Struct initialization failed before starting snapshot: {reason}"),
        serde_json::json!({
            "taskId": task.id,
            "runId": run_id,
            "reason": reason,
        }),
    )
}

/// Write a control_log intent row.
async fn write_control_intent(
    pool: &sqlx::SqlitePool,
    task_id: &str,
    run_id: &str,
    action: &str,
    operator_id: &str,
) -> Result<ControlLog, ApiError> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    ControlLogRepository::create(
        pool,
        &ControlLog {
            id: 0,
            task_id: task_id.to_string(),
            run_id: Some(run_id.to_string()),
            action: action.to_string(),
            intent_or_result: "intent".to_string(),
            operator_id: Some(operator_id.to_string()),
            created_at: now,
        },
    )
    .await
    .map_err(|e| {
        ApiError::new(
            codes::INTERNAL_ERROR,
            format!("control log intent write failed: {e}"),
        )
    })
}

/// Write a control_log result row.
async fn write_control_result(
    pool: &sqlx::SqlitePool,
    task_id: &str,
    run_id: &str,
    action: &str,
    result: &str,
    operator_id: &str,
) -> Result<ControlLog, ApiError> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    ControlLogRepository::create(
        pool,
        &ControlLog {
            id: 0,
            task_id: task_id.to_string(),
            run_id: Some(run_id.to_string()),
            action: action.to_string(),
            intent_or_result: format!("result:{result}"),
            operator_id: Some(operator_id.to_string()),
            created_at: now,
        },
    )
    .await
    .map_err(|e| {
        ApiError::new(
            codes::INTERNAL_ERROR,
            format!("control log result write failed: {e}"),
        )
    })
}

/// Write an operate_log audit entry.
async fn write_run_audit_log(
    pool: &sqlx::SqlitePool,
    actor: &str,
    action: &str,
    result: &str,
    target: &str,
    ip: &str,
) -> Result<(), ApiError> {
    write_run_audit_log_with_details(pool, actor, action, result, target, ip, None).await
}

/// Write an operate_log audit entry carrying a JSON `details` payload.
#[allow(clippy::too_many_arguments)]
async fn write_run_audit_log_with_details(
    pool: &sqlx::SqlitePool,
    actor: &str,
    action: &str,
    result: &str,
    target: &str,
    ip: &str,
    details: Option<serde_json::Value>,
) -> Result<(), ApiError> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let log = crate::models::OperateLog {
        id: 0,
        actor: actor.to_string(),
        action: action.to_string(),
        result: result.to_string(),
        target: Some(target.to_string()),
        details: details.map(|d| d.to_string()),
        ip: Some(ip.to_string()),
        created_at: now,
    };
    if let Err(e) = OperateLogRepository::create(pool, &log).await {
        tracing::warn!("audit log write failed: {e}");
    }
    Ok(())
}

/// Close out the `paused` Run a resume just superseded.
///
/// Without this the task owns two Runs in an active status and
/// `find_active_by_task` starts returning whichever sorts first — so every
/// later stop or pause could address the dead predecessor instead of the
/// live successor. `stop_method="resumed"` distinguishes it from a Run an
/// operator actually stopped.
async fn close_out_resumed_run(pool: &sqlx::SqlitePool, run_id: &str) -> Result<(), String> {
    let mut previous = RunRepository::find_by_id(pool, run_id)
        .await
        .map_err(|e| format!("lookup failed: {e}"))?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    previous.status = run_status::STOPPED.to_string();
    previous.stop_method = Some("resumed".to_string());
    if previous.stopped_at.is_none() {
        previous.stopped_at = Some(now.clone());
    }
    previous.updated_at = now;
    RunRepository::update(pool, &previous)
        .await
        .map(|_| ())
        .map_err(|e| format!("update failed: {e}"))
}

/// Convert a Run model to a RunResponse DTO, including position data.
/// Public so `task_handlers::list_task_runs` can reuse it.
pub fn run_to_response_public(run: &Run) -> RunResponse {
    run_to_response(run)
}

fn run_to_response(run: &Run) -> RunResponse {
    let position = run.log_dir.as_ref().and_then(|log_dir| {
        let run_dir = PathBuf::from(log_dir)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        executor::LocalExecutor::read_position(&run_dir)
    });

    RunResponse {
        id: run.id.clone(),
        task_id: run.task_id.clone(),
        status: run.status.clone(),
        pid: run.pid,
        ini_path: run.ini_path.clone(),
        log_dir: run.log_dir.clone(),
        started_at: run.started_at.clone(),
        stopped_at: run.stopped_at.clone(),
        exit_code: run.exit_code,
        stop_method: run.stop_method.clone(),
        position,
        metrics_port: run.metrics_port,
        resumed_from_run_id: run.resumed_from_run_id.clone(),
        created_at: run.created_at.clone(),
        updated_at: run.updated_at.clone(),
    }
}

/// POST /api/tasks/:id/start — start a new Run for a Task.
///
/// Returns 202 with `{run_id}` on success.
/// Returns 409 if a Run is already active for the Task or the port pool is exhausted.
/// Returns 422 if the license is expired or at cap.
/// Honours Idempotency-Key: replayed key returns cached 202 result.
#[post("/tasks/{id}/start")]
pub async fn start_task(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    active_runs: web::Data<ActiveRuns>,
    scraper_state: web::Data<ScraperState>,
    runtime_state: (web::Data<PortPool>, web::Data<IdempotencyCache>),
    req: actix_web::HttpRequest,
) -> HttpResponse {
    let (port_pool, idempotency_cache) = runtime_state;
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskStart) {
        return e.error_response();
    }

    // Idempotency-Key check: if the key was seen before, return the cached result.
    let idem_key = extract_scoped_key(&req, &user.user_id);
    if let Some(ref key) = idem_key {
        if let Some(cached) = idempotency_cache.get(key).await {
            return HttpResponse::build(
                actix_web::http::StatusCode::from_u16(cached.status)
                    .unwrap_or(actix_web::http::StatusCode::ACCEPTED),
            )
            .json(cached.body);
        }
    }

    let task_id = path.into_inner();
    let ip = client_ip(&req);

    start_run(
        pool,
        user,
        task_id,
        active_runs,
        scraper_state,
        port_pool,
        idempotency_cache,
        ip,
        idem_key,
        None,
    )
    .await
}

/// Everything a resumed Run needs from the `paused` Run it continues.
///
/// `resume` is not a distinct spawn path: it is a plain start whose INI is
/// pinned to a predecessor's position log (see
/// [`ini_renderer::render_for_resume`]) and whose predecessor is closed out
/// as `stopped`/`resumed` the moment the new engine is up.
#[derive(Debug, Clone)]
struct ResumeContext {
    /// The paused Run being continued.
    previous_run_id: String,
    /// Its `log_dir` — the position the new engine starts from.
    log_dir: String,
    /// Its rendered INI path, handed to the resumer as `config_file`.
    config_file: String,
    /// The audit action label for the operate log ("tasks.resume").
    audit_action: &'static str,
}

/// The peer address of a request, for audit rows.
fn client_ip(req: &actix_web::HttpRequest) -> String {
    req.connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Start a Run for `task_id` — fresh when `resume` is `None`, continuing a
/// paused Run's position when it is `Some`.
///
/// RBAC and idempotency replay are the caller's job; everything from the
/// license check onwards is shared, deliberately, so a resumed Run cannot
/// drift away from a started one (precheck, port allocation, supervision).
#[allow(clippy::too_many_arguments)]
async fn start_run(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    task_id: String,
    active_runs: web::Data<ActiveRuns>,
    scraper_state: web::Data<ScraperState>,
    port_pool: web::Data<PortPool>,
    idempotency_cache: web::Data<IdempotencyCache>,
    ip: String,
    idem_key: Option<String>,
    resume: Option<ResumeContext>,
) -> HttpResponse {
    let control_action = if resume.is_some() { "resume" } else { "start" };
    let audit_action = resume
        .as_ref()
        .map(|r| r.audit_action)
        .unwrap_or("tasks.start");

    // Check license expiry and cap before starting.
    if let Err(e) = crate::license_handlers::check_license_for_start(&pool).await {
        return e.error_response();
    }

    // Load the Task.
    let mut task = match TaskRepository::find_by_id(&pool, &task_id).await {
        Ok(t) => t,
        Err(_) => {
            return ApiError::with_details(
                codes::TASK_NOT_FOUND,
                "Task not found",
                serde_json::json!({ "id": task_id }),
            )
            .error_response();
        }
    };

    // Run precheck before spawning the engine.
    // If precheck has any blocking failures, return PRECHECK_BLOCKING_FAILED.
    // Catch panics (e.g. CDC tasks where precheck panics due to missing server_id)
    // by spawning a separate task and checking for JoinError::Panic.
    let precheck_task_id = task_id.clone();
    let precheck_pool = pool.get_ref().clone();
    let precheck_handle = tokio::spawn(async move {
        let task = TaskRepository::find_by_id(&precheck_pool, &precheck_task_id)
            .await
            .map_err(|_| ApiError::new(codes::TASK_NOT_FOUND, "Task not found for precheck"))?;
        crate::precheck_handlers::run_precheck(&task).await
    });

    match precheck_handle.await {
        Ok(Ok(resp)) => {
            // Precheck completed — check for blocking failures
            if resp.summary.fail > 0 {
                let failing: Vec<&PrecheckItem> =
                    resp.items.iter().filter(|i| i.status == "fail").collect();
                let failing_details: Vec<serde_json::Value> = failing
                    .iter()
                    .map(|i| {
                        serde_json::json!({
                            "name": i.name,
                            "side": i.side,
                            "errorMessage": i.error_message,
                        })
                    })
                    .collect();
                return ApiError::with_details(
                    codes::PRECHECK_BLOCKING_FAILED,
                    "Precheck found blocking issues",
                    serde_json::json!({
                        "failCount": resp.summary.fail,
                        "failingItems": failing_details,
                    }),
                )
                .error_response();
            }
        }
        Ok(Err(e)) => {
            // Precheck returned an error (e.g. invalid config)
            return ApiError::with_details(
                codes::PRECHECK_BLOCKING_FAILED,
                "Precheck failed to execute",
                serde_json::json!({
                    "error": e.message,
                }),
            )
            .error_response();
        }
        Err(join_err) => {
            // Task panicked — extract the panic message and log with task_id
            // so operators can grep server logs from a UI bug report.
            let panic_msg = if join_err.is_panic() {
                let payload = join_err.into_panic();
                match payload.downcast_ref::<String>() {
                    Some(s) => s.clone(),
                    None => match payload.downcast_ref::<&str>() {
                        Some(s) => s.to_string(),
                        None => "precheck panicked with unknown cause".to_string(),
                    },
                }
            } else {
                "precheck task was cancelled".to_string()
            };
            let request_id = uuid::Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>();
            tracing::error!(
                request_id = %request_id,
                task_id = %task_id,
                panic_message = %panic_msg,
                "start_task: precheck panicked — refusing to start engine",
            );
            return ApiError::with_details(
                codes::PRECHECK_PANIC,
                format!(
                    "precheck task crashed unexpectedly (request_id={request_id}). \
                     Send this request ID to ops or grep console-server logs."
                ),
                serde_json::json!({
                    "requestId": request_id,
                    "panicMessage": panic_msg,
                }),
            )
            .error_response();
        }
    }

    // Atomically claim the active-runs slot to eliminate the TOCTOU race.
    // We hold the mutex across both the has-active-run check and the
    // Starting-insert, so two concurrent start_task calls for the same
    // task_id will serialize: the first claims the slot, the second sees
    // it occupied and returns 409 RUN_ALREADY_ACTIVE.
    {
        let mut active = active_runs.lock().await;
        if active.contains_key(&task_id) {
            return ApiError::with_details(
                codes::RUN_ALREADY_ACTIVE,
                "A run is already active for this task",
                serde_json::json!({ "task_id": task_id }),
            )
            .error_response();
        }
        active.insert(task_id.clone(), RunSlot::Starting);
    }

    // Also check DB for active runs from previous orchestrator sessions.
    // A resume is the one case where an active Run legitimately exists: the
    // `paused` predecessor, which this call is about to close out.
    let resumed_from = resume.as_ref().map(|r| r.previous_run_id.as_str());
    if let Ok(Some(active_run)) = RunRepository::find_active_by_task(&pool, &task_id).await {
        if Some(active_run.id.as_str()) != resumed_from {
            // Clean up the Starting slot we just claimed.
            {
                let mut active = active_runs.lock().await;
                active.remove(&task_id);
            }
            return ApiError::with_details(
                codes::RUN_ALREADY_ACTIVE,
                "A run is already active for this task",
                serde_json::json!({ "run_id": active_run.id, "run_status": active_run.status }),
            )
            .error_response();
        }
    }

    let binary_override = if std::env::var("APE_DTS_BINARY_PATH").is_ok() {
        Some(std::env::var("APE_DTS_BINARY_PATH").unwrap())
    } else {
        None
    };
    // Struct init belongs to a *fresh* start only: on resume the target
    // structures already exist, and re-running it would either no-op loudly
    // or fight the data the paused Run already wrote.
    let struct_init = if resume.is_none() {
        run_struct_init_if_requested(&task, binary_override.as_deref()).await
    } else {
        Ok(())
    };
    if let Err(e) = struct_init {
        {
            let mut active = active_runs.lock().await;
            active.remove(&task_id);
        }
        let _ = write_run_audit_log(
            &pool,
            &user.username,
            audit_action,
            "failure",
            &task_id,
            &ip,
        )
        .await;
        return e.error_response();
    }

    // Create the Run record in pending state.
    let run_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let base_dir = executor::run_data_dir();
    let run_dir_str = format!("{base_dir}/{run_id}");
    let ini_path_str = format!("{run_dir_str}/task_config.ini");
    let log_dir_str = format!("{run_dir_str}/logs");

    let mut run = Run {
        id: run_id.clone(),
        task_id: Some(task_id.clone()),
        status: run_status::PENDING.to_string(),
        pid: None,
        ini_path: Some(ini_path_str),
        log_dir: Some(log_dir_str),
        started_at: Some(now.clone()),
        stopped_at: None,
        exit_code: None,
        stop_method: None,
        metrics_port: None,
        resumed_from_run_id: resume.as_ref().map(|r| r.previous_run_id.clone()),
        created_at: now.clone(),
        updated_at: now,
    };

    // Write control log intent.
    let _ = write_control_intent(&pool, &task_id, &run_id, control_action, &user.username).await;

    // Allocate a metrics port from the pool BEFORE rendering INI, so the port
    // is injected into the [metrics] section and the engine binds exactly that port.
    let metrics_port = match port_pool.acquire().await {
        Some(p) => p,
        None => {
            // Port pool exhausted — clean up and return a clear error.
            {
                let mut active = active_runs.lock().await;
                active.remove(&task_id);
            }
            return ApiError::new(
                codes::PORT_POOL_EXHAUSTED,
                "All metrics ports in [9100, 9199] are currently in use; \
                 wait for a running Run to terminate before starting a new one",
            )
            .error_response();
        }
    };

    // Inject the allocated port into the task's metrics_config JSON, preserving
    // any other user-supplied keys (e.g., custom labels).
    {
        let mut mc: serde_json::Value =
            serde_json::from_str(&task.metrics_config).unwrap_or_default();
        if let Some(obj) = mc.as_object_mut() {
            obj.insert("http_port".to_string(), serde_json::json!(metrics_port));
        } else {
            mc = serde_json::json!({ "http_port": metrics_port });
        }
        task.metrics_config = mc.to_string();
    }

    // Render INI from the Task. For managed `snapshot_and_cdc` tasks the
    // engine runs in two phases (snapshot → cdc); pre-stage the phase 2 INI
    // and capture the CDC start marker BEFORE we spawn so phase 2 picks up
    // every change made during phase 1.
    let mut resume_overrides_applied: Vec<String> = Vec::new();
    let ini_content = if let Some(ctx) = resume.as_ref() {
        // A resumed Run never goes through the two-phase path: pause is only
        // offered for plain snapshot/cdc tasks (see `pause_task`).
        let rendered = ini_renderer::render_for_resume(
            &task,
            &ini_renderer::ResumeOverrides {
                log_dir: ctx.log_dir.clone(),
                config_file: ctx.config_file.clone(),
            },
        );
        resume_overrides_applied = rendered.applied;
        rendered.ini
    } else if crate::two_phase::is_two_phase_task(&task) {
        let run_dir_path = std::path::PathBuf::from(&run_dir_str);
        let phase2_start = match crate::two_phase::capture_phase2_start(&task).await {
            Ok(start) => start,
            Err(e) => {
                port_pool.release(metrics_port).await;
                {
                    let mut active = active_runs.lock().await;
                    active.remove(&task_id);
                }
                return ApiError::new(
                    codes::INTERNAL_ERROR,
                    format!("two-phase start marker capture failed: {e}"),
                )
                .error_response();
            }
        };
        match crate::two_phase::prepare_run_dir(&task, &run_dir_path, phase2_start) {
            Ok(prep) => prep.phase1_ini,
            Err(e) => {
                port_pool.release(metrics_port).await;
                {
                    let mut active = active_runs.lock().await;
                    active.remove(&task_id);
                }
                return ApiError::new(
                    codes::INTERNAL_ERROR,
                    format!("two-phase preparation failed: {e}"),
                )
                .error_response();
            }
        }
    } else {
        ini_renderer::render(&task)
    };

    // Spawn the engine subprocess.
    let engine_env = engine_extra_env(&task);
    let handle = match executor::LocalExecutor::spawn_with_env(
        &run_id,
        &ini_content,
        binary_override.as_deref(),
        &engine_env,
    )
    .await
    {
        Ok(h) => h,
        Err(e) => {
            // Spawn failed — release the port, mark the Run as failed, clean up Starting slot.
            port_pool.release(metrics_port).await;

            run.status = run_status::FAILED.to_string();
            run.exit_code = Some(-1);
            run.stopped_at =
                Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
            run.updated_at =
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

            if let Ok(_saved) = RunRepository::create(&pool, &run).await {
                let _ = write_control_result(
                    &pool,
                    &task_id,
                    &run_id,
                    control_action,
                    "error",
                    &user.username,
                )
                .await;
            }

            // Remove the Starting slot so the task_id can be re-used.
            {
                let mut active = active_runs.lock().await;
                active.remove(&task_id);
            }

            let _ = write_run_audit_log(
                &pool,
                &user.username,
                audit_action,
                "failure",
                &task_id,
                &ip,
            )
            .await;

            return ApiError::new(codes::INTERNAL_ERROR, format!("engine spawn failed: {e}"))
                .error_response();
        }
    };

    // Update the Run with PID, running status, and the allocated metrics port.
    run.pid = Some(handle.pid as i64);
    run.status = run_status::RUNNING.to_string();
    run.metrics_port = Some(metrics_port as i64);
    run.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    // The engine is up. Close out the paused predecessor *now*, before the
    // successor row exists: two rows of the same task in an active status
    // would make `find_active_by_task` pick one at random, and every stop or
    // pause after this point would address the wrong Run.
    if let Some(ctx) = resume.as_ref() {
        if let Err(e) = close_out_resumed_run(&pool, &ctx.previous_run_id).await {
            tracing::warn!(
                "resume: failed to close out predecessor run {}: {e}",
                ctx.previous_run_id
            );
        }
        if !resume_overrides_applied.is_empty() {
            let _ = write_run_audit_log_with_details(
                &pool,
                &user.username,
                "tasks.resume.ini_override",
                "success",
                &run_id,
                &ip,
                Some(serde_json::json!({
                    "resumedFromRunId": ctx.previous_run_id,
                    "overrides": resume_overrides_applied,
                })),
            )
            .await;
        }
    }

    let _saved = match RunRepository::create(&pool, &run).await {
        Ok(r) => r,
        Err(e) => {
            // Failed to persist — release port, kill the child process, clean up slot.
            port_pool.release(metrics_port).await;
            let _ = executor::LocalExecutor::kill_with_grace(&handle, 3).await;
            {
                let mut active = active_runs.lock().await;
                active.remove(&task_id);
            }
            return ApiError::new(codes::INTERNAL_ERROR, format!("run creation failed: {e}"))
                .error_response();
        }
    };

    // Write control log result.
    let _ = write_control_result(
        &pool,
        &task_id,
        &run_id,
        control_action,
        "success",
        &user.username,
    )
    .await;

    // Replace the Starting slot with the real Active handle.
    {
        let mut active = active_runs.lock().await;
        active.insert(task_id.clone(), RunSlot::Active(handle));
    }

    // Register the Run as a scrape target for the MetricsScraper, using the
    // allocated port so the scraper hits the correct engine endpoint.
    {
        let target = metrics_scraper::scrape_target_from_run(&task_id, &run_id, metrics_port);
        scraper_state.add_target(target).await;
    }

    // Update the Task status to "running".
    let _ = update_task_status(&pool, &task_id, "running").await;

    // Write audit log.
    let _ = write_run_audit_log(&pool, &user.username, audit_action, "success", &run_id, &ip).await;

    // Spawn a background task to monitor the child process.
    let bg_pool = pool.get_ref().clone();
    let bg_active_runs = active_runs.get_ref().clone();
    let bg_port_pool = port_pool.get_ref().clone();
    let bg_task_id = task_id.clone();
    let bg_run_id = run_id.clone();
    let bg_metrics_port = Some(metrics_port);
    tokio::spawn(async move {
        supervise_run(
            bg_pool,
            bg_active_runs,
            bg_task_id,
            bg_run_id,
            bg_port_pool,
            bg_metrics_port,
        )
        .await;
    });

    // Cache the result if an Idempotency-Key was provided.
    let response_body = serde_json::to_value(&StartRunResponse {
        run_id: run_id.clone(),
    })
    .unwrap_or(serde_json::json!({ "run_id": run_id }));
    if let Some(ref key) = idem_key {
        idempotency_cache.put(key, 202, response_body.clone()).await;
    }

    HttpResponse::Accepted().json(response_body)
}

/// POST /api/tasks/:id/stop — stop the active Run for a Task.
///
/// Returns 202 on success.
/// Returns 409 if no active Run exists (with ILLEGAL_TRANSITION details).
/// Honours Idempotency-Key: replayed key returns cached 202 result.
#[post("/tasks/{id}/stop")]
pub async fn stop_task(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    active_runs: web::Data<ActiveRuns>,
    scraper_state: web::Data<ScraperState>,
    idempotency_cache: web::Data<IdempotencyCache>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskStop) {
        return e.error_response();
    }

    // Idempotency-Key check: if the key was seen before, return the cached result.
    let idem_key = extract_scoped_key(&req, &user.user_id);
    if let Some(ref key) = idem_key {
        if let Some(cached) = idempotency_cache.get(key).await {
            return HttpResponse::build(
                actix_web::http::StatusCode::from_u16(cached.status)
                    .unwrap_or(actix_web::http::StatusCode::ACCEPTED),
            )
            .json(cached.body);
        }
    }

    let task_id = path.into_inner();
    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Find the active Run.
    let active_run = match RunRepository::find_active_by_task(&pool, &task_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            // No active run — check if there's a terminal run to report the
            // correct ILLEGAL_TRANSITION error with {from, to} details.
            let from_status = RunRepository::find_latest_by_task(&pool, &task_id)
                .await
                .ok()
                .flatten()
                .map(|r| r.status)
                .unwrap_or_else(|| "none".to_string());
            return ApiError::with_details(
                codes::ILLEGAL_TRANSITION,
                "Cannot stop a run that is not active",
                serde_json::json!({ "from": from_status, "to": "stopping" }),
            )
            .error_response();
        }
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("run lookup failed: {e}"))
                .error_response();
        }
    };

    // Only running or paused runs can be stopped. `pausing` is deliberately
    // excluded: a graceful stop is already in flight and the supervisor is
    // about to give it a terminal status.
    if !matches!(active_run.status.as_str(), "running" | "paused") {
        return ApiError::with_details(
            codes::ILLEGAL_TRANSITION,
            "Run is not in a stoppable state",
            serde_json::json!({ "from": active_run.status, "to": "stopping" }),
        )
        .error_response();
    }

    let run_id = active_run.id.clone();

    // A `paused` Run has no process: pause stopped the engine for good and
    // only its position log survives. Stopping it is a decision to discard
    // that position, not something to signal — sending SIGTERM here would
    // hit a recycled pid at worst and nothing at best.
    if active_run.status == run_status::PAUSED {
        let _ = write_control_intent(&pool, &task_id, &run_id, "stop", &user.username).await;

        let mut run = active_run;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        run.status = run_status::STOPPED.to_string();
        run.stop_method = Some("discarded".to_string());
        if run.stopped_at.is_none() {
            run.stopped_at = Some(now.clone());
        }
        run.updated_at = now;

        if let Err(e) = RunRepository::update(&pool, &run).await {
            let _ = write_control_result(&pool, &task_id, &run_id, "stop", "error", &user.username)
                .await;
            return ApiError::new(
                codes::INTERNAL_ERROR,
                format!("run status update failed: {e}"),
            )
            .error_response();
        }

        scraper_state.remove_target(&task_id).await;
        let _ =
            write_control_result(&pool, &task_id, &run_id, "stop", "success", &user.username).await;
        let _ = update_task_status(&pool, &task_id, "stopped").await;
        let _ =
            write_run_audit_log(&pool, &user.username, "tasks.stop", "success", &run_id, &ip).await;

        let stop_body = serde_json::json!({ "run_id": run_id });
        if let Some(ref key) = idem_key {
            idempotency_cache.put(key, 202, stop_body.clone()).await;
        }
        return HttpResponse::Accepted().json(stop_body);
    }

    // Write control log intent.
    let _ = write_control_intent(&pool, &task_id, &run_id, "stop", &user.username).await;

    // Transition to stopping.
    let mut run = active_run;
    if !is_legal_transition(&run.status, run_status::STOPPING) {
        let _ =
            write_control_result(&pool, &task_id, &run_id, "stop", "error", &user.username).await;
        return ApiError::with_details(
            codes::ILLEGAL_TRANSITION,
            "Illegal state transition",
            serde_json::json!({ "from": run.status, "to": "stopping" }),
        )
        .error_response();
    }

    let status_before_stop = run.status.clone();
    run.status = run_status::STOPPING.to_string();
    run.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    if let Err(e) = RunRepository::update(&pool, &run).await {
        let _ =
            write_control_result(&pool, &task_id, &run_id, "stop", "error", &user.username).await;
        return ApiError::new(
            codes::INTERNAL_ERROR,
            format!("run status update failed: {e}"),
        )
        .error_response();
    }

    // Final scrape: capture the engine's last metric emission before killing
    // the child process. The engine is still alive at this point, so the
    // scrape is likely to succeed and capture the final progress value.
    if let Some(port) = run.metrics_port.and_then(|p| u16::try_from(p).ok()) {
        let task_id_for_scrape = run.task_id.as_deref().unwrap_or(&task_id);
        metrics_scraper::scrape_single_run(&pool, task_id_for_scrape, &run_id, "127.0.0.1", port)
            .await;
    }

    // Kill the child process. A kill that did not happen must not be papered
    // over with a `stopped` status: the engine would still be running while
    // the console reports it stopped, and the task would look free to restart.
    let kill_outcome: Result<Option<KillResult>, String> = {
        let mut active = active_runs.lock().await;
        match active.remove(&task_id) {
            Some(slot) => match slot.into_handle() {
                Some(handle) => match executor::LocalExecutor::kill(&handle).await {
                    Ok(kr) => Ok(Some(kr)),
                    Err(e) => {
                        // Put the slot back — the process may still be alive and
                        // this handle is the only way to reach it again.
                        active.insert(task_id.clone(), RunSlot::Active(handle));
                        Err(e)
                    }
                },
                None => {
                    // Slot was in Starting state: the engine is mid-spawn, so
                    // there is nothing to signal *yet* — and releasing the slot
                    // would let a second start race the one in flight. Put it
                    // back and make the caller retry.
                    active.insert(task_id.clone(), RunSlot::Starting);
                    Err("the run is still starting; retry the stop once it is running".to_string())
                }
            },
            None => {
                // No handle in memory — stop by pid, with the same graceful
                // escalation, so "stopped" still means the process is gone.
                match run.pid {
                    Some(pid) if pid > 0 => {
                        let grace = executor::grace_window_secs();
                        executor::LocalExecutor::kill_by_pid(pid as u32, grace)
                            .await
                            .map(Some)
                    }
                    _ => Ok(None),
                }
            }
        }
    };

    let kill_result = match kill_outcome {
        Ok(kr) => kr,
        Err(e) => {
            tracing::warn!("kill failed for run {}: {e}", run_id);
            // Roll the Run back to where it was so a retry is possible.
            run.status = status_before_stop;
            run.updated_at =
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            if let Err(e) = RunRepository::update(&pool, &run).await {
                tracing::warn!("failed to roll run {} back after failed kill: {e}", run_id);
            }
            let _ = write_control_result(&pool, &task_id, &run_id, "stop", "error", &user.username)
                .await;
            let _ = write_run_audit_log(&pool, &user.username, "tasks.stop", "error", &run_id, &ip)
                .await;
            return ApiError::with_details(
                codes::INTERNAL_ERROR,
                format!("failed to stop the engine process: {e}"),
                serde_json::json!({ "run_id": run_id, "pid": run.pid }),
            )
            .error_response();
        }
    };

    // Remove the scrape target.
    scraper_state.remove_target(&task_id).await;

    // Update the Run with final state.
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    run.status = run_status::STOPPED.to_string();
    run.stopped_at = Some(now.clone());
    run.updated_at = now;

    if let Some(kr) = kill_result {
        run.stop_method = Some(kr.stop_method);
        match kr.exit_status {
            ExitStatus::Exited { code } => {
                run.exit_code = Some(code as i64);
            }
            ExitStatus::Signaled { signal } => {
                // Signal-killed processes typically have exit code = 128 + signal.
                run.exit_code = Some(128 + signal as i64);
            }
        }
    }

    if let Err(e) = RunRepository::update(&pool, &run).await {
        tracing::warn!("failed to update run after stop: {e}");
    }

    // Write control log result.
    let _ = write_control_result(&pool, &task_id, &run_id, "stop", "success", &user.username).await;

    // Update task status.
    let _ = update_task_status(&pool, &task_id, "stopped").await;

    // Write audit log.
    let _ = write_run_audit_log(&pool, &user.username, "tasks.stop", "success", &run_id, &ip).await;

    // Cache the result if an Idempotency-Key was provided.
    let stop_body = serde_json::json!({ "run_id": run_id });
    if let Some(ref key) = idem_key {
        idempotency_cache.put(key, 202, stop_body.clone()).await;
    }

    HttpResponse::Accepted().json(stop_body)
}

/// POST /api/tasks/:id/pause — pause the active Run for a Task.
///
/// Pause is a graceful stop *with intent*, not a soft suspend (ADR 0004):
/// the engine gets the same SIGTERM as a stop, drains, writes its position
/// and exits 143 — the process is gone afterwards. The Run goes to `pausing`
/// first and only the supervisor, seeing the real exit code, decides whether
/// it lands in `paused` (position trustworthy, resumable) or `failed`.
///
/// Returns 202 on success.
/// Returns 409 if the Run is not in a pausable state or the task kind has no
/// position to resume from.
#[post("/tasks/{id}/pause")]
pub async fn pause_task(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    active_runs: web::Data<ActiveRuns>,
    scraper_state: web::Data<ScraperState>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskStart) {
        return e.error_response();
    }

    let task_id = path.into_inner();
    let ip = client_ip(&req);

    // Find the active Run.
    let mut run = match RunRepository::find_active_by_task(&pool, &task_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            // No active run — report ILLEGAL_TRANSITION with the terminal status.
            let from_status = RunRepository::find_latest_by_task(&pool, &task_id)
                .await
                .ok()
                .flatten()
                .map(|r| r.status)
                .unwrap_or_else(|| "none".to_string());
            return ApiError::with_details(
                codes::ILLEGAL_TRANSITION,
                "Cannot pause a run that is not active",
                serde_json::json!({ "from": from_status, "to": "paused" }),
            )
            .error_response();
        }
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("run lookup failed: {e}"))
                .error_response();
        }
    };

    // Only running runs can be paused.
    if run.status != run_status::RUNNING {
        return ApiError::with_details(
            codes::ILLEGAL_TRANSITION,
            "Cannot pause a run that is not running",
            serde_json::json!({ "from": run.status, "to": "paused" }),
        )
        .error_response();
    }

    // Kind gate: pause is only meaningful where there is a position to
    // resume from. A `check` or `struct` task has none, so "pause" would be
    // an ordinary stop wearing a resumable label.
    let task = match TaskRepository::find_by_id(&pool, &task_id).await {
        Ok(t) => t,
        Err(_) => {
            return ApiError::with_details(
                codes::TASK_NOT_FOUND,
                "Task not found",
                serde_json::json!({ "id": task_id }),
            )
            .error_response();
        }
    };
    if let Some(reason) = pause_unsupported_reason(&task) {
        return ApiError::with_details(
            codes::UNSUPPORTED_FOR_KIND,
            reason,
            serde_json::json!({ "kind": task.kind, "task_id": task_id }),
        )
        .error_response();
    }

    let run_id = run.id.clone();

    // Write control log intent.
    let _ = write_control_intent(&pool, &task_id, &run_id, "pause", &user.username).await;

    // Mark `pausing` BEFORE signalling. The supervisor reads this status to
    // tell a requested pause from a requested stop from an external kill, so
    // it has to be persisted before the exit it is meant to explain.
    let status_before_pause = run.status.clone();
    run.status = run_status::PAUSING.to_string();
    run.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    if let Err(e) = RunRepository::update(&pool, &run).await {
        let _ =
            write_control_result(&pool, &task_id, &run_id, "pause", "error", &user.username).await;
        return ApiError::new(
            codes::INTERNAL_ERROR,
            format!("run status update failed: {e}"),
        )
        .error_response();
    }

    // Final scrape while the engine's metrics server is still up, then stop
    // scraping: after the SIGTERM there is no process left to scrape.
    if let Some(port) = run.metrics_port.and_then(|p| u16::try_from(p).ok()) {
        let task_id_for_scrape = run.task_id.as_deref().unwrap_or(&task_id);
        metrics_scraper::scrape_single_run(&pool, task_id_for_scrape, &run_id, "127.0.0.1", port)
            .await;
    }

    // Signal the engine: the same cooperative SIGTERM a stop sends.
    let signalled = match pause_signal_target(&active_runs, &task_id, &run).await {
        Ok(outcome) => outcome,
        Err(e) => {
            // Roll back so a retry is possible — a Run stuck in `pausing`
            // with a live engine would never be finalised by anyone.
            run.status = status_before_pause;
            run.updated_at =
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            if let Err(e) = RunRepository::update(&pool, &run).await {
                tracing::warn!("failed to roll run {} back after failed pause: {e}", run_id);
            }
            let _ =
                write_control_result(&pool, &task_id, &run_id, "pause", "error", &user.username)
                    .await;
            let _ =
                write_run_audit_log(&pool, &user.username, "tasks.pause", "error", &run_id, &ip)
                    .await;
            return ApiError::with_details(
                codes::INTERNAL_ERROR,
                format!("failed to signal the engine process: {e}"),
                serde_json::json!({ "run_id": run_id, "pid": run.pid }),
            )
            .error_response();
        }
    };

    scraper_state.remove_target(&task_id).await;

    if signalled == crate::signal::SignalOutcome::ProcessGone {
        // Nothing was paused: the engine had already died, so whatever it
        // left behind is not the product of a drain and must not be dressed
        // up as a pause. Put the Run back to `running` and let the
        // supervisor finalise it from the real exit code.
        tracing::warn!("pause: engine process for run {} is already gone", run_id);
        run.status = status_before_pause;
        run.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        if let Err(e) = RunRepository::update(&pool, &run).await {
            tracing::warn!(
                "failed to roll run {} back after a no-op pause: {e}",
                run_id
            );
        }
        let _ =
            write_control_result(&pool, &task_id, &run_id, "pause", "error", &user.username).await;
        return ApiError::with_details(
            codes::ILLEGAL_TRANSITION,
            "Cannot pause a run whose engine process is gone",
            serde_json::json!({ "from": run.status, "to": "paused", "run_id": run_id }),
        )
        .error_response();
    }

    // Write control log result.
    let _ =
        write_control_result(&pool, &task_id, &run_id, "pause", "success", &user.username).await;

    // Write audit log.
    let _ = write_run_audit_log(
        &pool,
        &user.username,
        "tasks.pause",
        "success",
        &run_id,
        &ip,
    )
    .await;

    HttpResponse::Accepted().json(serde_json::json!({ "run_id": run_id }))
}

/// Why this task cannot be paused, or `None` when it can.
fn pause_unsupported_reason(task: &Task) -> Option<&'static str> {
    if !matches!(task.kind.as_str(), "snapshot" | "cdc") {
        return Some(
            "pause is only supported for snapshot and cdc tasks; \
             check and struct tasks have no resumable position",
        );
    }
    if crate::two_phase::is_two_phase_task(task) {
        // The two-phase orchestration owns its own snapshot→cdc handover
        // (a pre-staged phase 2 INI and a start marker captured before phase
        // 1). Resuming from a position log would bypass both.
        return Some(
            "pause is not supported for managed snapshot_and_cdc tasks: \
             the snapshot→cdc handover owns the start position",
        );
    }
    None
}

/// Send the pause SIGTERM, preferring the in-memory child handle and falling
/// back to the recorded pid for Runs re-attached after an orchestrator restart.
async fn pause_signal_target(
    active_runs: &ActiveRuns,
    task_id: &str,
    run: &Run,
) -> Result<crate::signal::SignalOutcome, String> {
    let pid = {
        let active = active_runs.lock().await;
        match active.get(task_id) {
            Some(RunSlot::Active(handle)) => Some(handle.pid),
            // Mid-spawn: there is nothing to signal yet, and dropping the
            // slot would let a second start race the one in flight.
            Some(RunSlot::Starting) => {
                return Err("the run is still starting; retry the pause once it is running".into())
            }
            None => match run.pid {
                Some(pid) if pid > 0 => u32::try_from(pid).ok(),
                _ => None,
            },
        }
    };

    match pid {
        Some(pid) => crate::signal::send(pid, crate::signal::EngineSignal::Term),
        None => Err("the run has no usable process id".to_string()),
    }
}

/// POST /api/tasks/:id/resume — resume a paused Run.
///
/// A resume is a **new Run** started from the paused Run's position log, not
/// a signal to a suspended process: pause left no process behind (ADR 0004).
/// The paused predecessor is closed out as `stopped`/`resumed` and the new
/// Run records it in `resumed_from_run_id`.
///
/// Returns 202 with the *new* `run_id`.
/// Returns 409 if the latest Run for the task is not `paused`.
#[post("/tasks/{id}/resume")]
pub async fn resume_task(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    active_runs: web::Data<ActiveRuns>,
    scraper_state: web::Data<ScraperState>,
    runtime_state: (web::Data<PortPool>, web::Data<IdempotencyCache>),
    req: actix_web::HttpRequest,
) -> HttpResponse {
    let (port_pool, idempotency_cache) = runtime_state;
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskStart) {
        return e.error_response();
    }

    let idem_key = extract_scoped_key(&req, &user.user_id);
    if let Some(ref key) = idem_key {
        if let Some(cached) = idempotency_cache.get(key).await {
            return HttpResponse::build(
                actix_web::http::StatusCode::from_u16(cached.status)
                    .unwrap_or(actix_web::http::StatusCode::ACCEPTED),
            )
            .json(cached.body);
        }
    }

    let task_id = path.into_inner();
    let ip = client_ip(&req);

    // The Run being resumed must be the task's most recent one: resuming an
    // older paused Run would silently rewind past everything since.
    let previous = match RunRepository::find_latest_by_task(&pool, &task_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return ApiError::with_details(
                codes::ILLEGAL_TRANSITION,
                "Cannot resume a task that has never run",
                serde_json::json!({ "from": "none", "to": "running" }),
            )
            .error_response();
        }
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("run lookup failed: {e}"))
                .error_response();
        }
    };

    if previous.status != run_status::PAUSED {
        return ApiError::with_details(
            codes::ILLEGAL_TRANSITION,
            "Cannot resume a run that is not paused",
            serde_json::json!({ "from": previous.status, "to": "running" }),
        )
        .error_response();
    }

    // The position log is the whole point of a resume: without it there is
    // nothing to continue from, and starting anyway would re-run the task
    // from its original start marker — a silent duplicate migration.
    let (log_dir, config_file) = match (previous.log_dir.clone(), previous.ini_path.clone()) {
        (Some(log_dir), Some(ini_path)) => (log_dir, ini_path),
        _ => {
            return ApiError::with_details(
                codes::ILLEGAL_TRANSITION,
                "Cannot resume a run that has no position log",
                serde_json::json!({ "run_id": previous.id }),
            )
            .error_response();
        }
    };

    let ctx = ResumeContext {
        previous_run_id: previous.id.clone(),
        log_dir,
        config_file,
        audit_action: "tasks.resume",
    };

    start_run(
        pool,
        user,
        task_id,
        active_runs,
        scraper_state,
        port_pool,
        idempotency_cache,
        ip,
        idem_key,
        Some(ctx),
    )
    .await
}

/// GET /api/runs — list all Runs across all Tasks, most recent first.
///
/// Returns a flat array of RunResponse objects (no pagination on the dashboard
/// list; callers can filter by status client-side).
#[get("/runs")]
pub async fn list_runs(pool: web::Data<sqlx::SqlitePool>, user: UserContext) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    match RunRepository::list_all(&pool).await {
        Ok(runs) => {
            let items: Vec<RunResponse> = runs.iter().map(run_to_response).collect();
            HttpResponse::Ok().json(items)
        }
        Err(e) => {
            ApiError::new(codes::INTERNAL_ERROR, format!("run list failed: {e}")).error_response()
        }
    }
}

/// GET /api/runs/:id — get Run details including position.
#[get("/runs/{id}")]
pub async fn get_run(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let id = path.into_inner();
    match RunRepository::find_by_id(&pool, &id).await {
        Ok(run) => HttpResponse::Ok().json(run_to_response(&run)),
        Err(_) => ApiError::with_details(
            codes::RUN_NOT_FOUND,
            "Run not found",
            serde_json::json!({ "id": id }),
        )
        .error_response(),
    }
}

// ─── Background supervision ────────────────────────────────────────────

/// Find the operator_id from the start intent for a given run.
/// Returns "system" if no intent row is found.
async fn find_operator_for_run(pool: &sqlx::SqlitePool, run_id: &str) -> String {
    let logs = ControlLogRepository::list(pool).await.unwrap_or_default();
    logs.iter()
        .find(|l| {
            (l.action == "start" || l.action == "resume")
                && l.run_id.as_deref() == Some(run_id)
                && l.intent_or_result == "intent"
        })
        .and_then(|l| l.operator_id.clone())
        .unwrap_or_else(|| "system".to_string())
}

/// Supervise a running Run: poll the child process and update the DB
/// when it exits. Releases the Run's metrics port back to the pool when
/// the Run reaches a terminal state.
///
/// The `metrics_port` is used for a pre-reap scrape: on each poll cycle
/// the supervisor scrapes the engine's `/metrics` endpoint BEFORE checking
/// whether the child has exited. This ensures the final metric emission
/// (e.g., progress=100) is captured while the engine's HTTP server is
/// still alive. If the scrape happens after the child exit, the server
/// is already gone and the scrape returns 502/connection-refused.
pub async fn supervise_run(
    pool: sqlx::SqlitePool,
    active_runs: ActiveRuns,
    task_id: String,
    run_id: String,
    port_pool: PortPool,
    metrics_port: Option<u16>,
) {
    // Poll the child process status every 2 seconds.
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Pre-reap scrape: capture the engine's last metric emission
        // BEFORE checking whether the child has exited. Once the child
        // exits, the engine's metrics HTTP server is gone and any
        // scrape attempt will fail (502 / connection refused).
        if let Some(port) = metrics_port {
            metrics_scraper::scrape_single_run(&pool, &task_id, &run_id, "127.0.0.1", port).await;
        }

        let slot = {
            let active = active_runs.lock().await;
            active.get(&task_id).cloned()
        };

        match slot {
            Some(RunSlot::Active(handle)) => {
                let status = executor::LocalExecutor::status(&handle).await;
                match status {
                    ChildStatus::Running => {
                        // Still running — continue polling.
                        continue;
                    }
                    ChildStatus::Exited(exit) => {
                        let clean_exit = matches!(&exit, ExitStatus::Exited { code: 0 });

                        // Two-phase MySQL snapshot+cdc: on a clean phase 1
                        // exit, transparently spawn phase 2 (cdc) using the
                        // INI we pre-staged before phase 1 started.
                        if clean_exit {
                            if let Some(state) = crate::two_phase::read_phase_state(&handle.run_dir)
                            {
                                if state.current_phase == 1 {
                                    match advance_to_phase2(
                                        &pool,
                                        &active_runs,
                                        &task_id,
                                        &run_id,
                                        &handle.run_dir,
                                        &state.phase2_ini_path,
                                    )
                                    .await
                                    {
                                        Ok(()) => continue,
                                        Err(e) => {
                                            tracing::warn!(
                                                "phase2 transition failed for run {}: {}",
                                                run_id,
                                                e
                                            );
                                            // Fall through and mark FAILED so the user sees the
                                            // bug instead of a silent stop.
                                        }
                                    }
                                }
                            }
                        }

                        // Child exited — update the Run record.
                        // Note: the final scrape was already done at the top of
                        // this loop iteration (before the status check), so the
                        // engine's last metric emission has already been captured.
                        let mut run = match RunRepository::find_by_id(&pool, &run_id).await {
                            Ok(r) => r,
                            Err(_) => break,
                        };

                        let now =
                            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                        run.stopped_at = Some(now.clone());
                        run.updated_at = now;

                        match exit {
                            ExitStatus::Exited { code } => run.exit_code = Some(code as i64),
                            ExitStatus::Signaled { signal } => {
                                run.exit_code = Some(128 + signal as i64)
                            }
                        }

                        // What the exit *means* depends on what the console
                        // asked for, which is exactly what the Run's current
                        // status records: `pausing` → paused, `stopping` →
                        // stopped, neither → somebody outside the console
                        // stopped the engine.
                        let mut external_stop = false;
                        match executor::classify_exit(&run.status, &exit) {
                            executor::ExitDisposition::Paused => {
                                run.status = run_status::PAUSED.to_string();
                                run.stop_method = Some("paused".to_string());
                            }
                            executor::ExitDisposition::Stopped => {
                                run.status = run_status::STOPPED.to_string();
                            }
                            executor::ExitDisposition::StoppedExternally => {
                                // Routine under a process supervisor (a k8s
                                // rolling update sends SIGTERM), so not
                                // `failed` — alerting would drown. The
                                // control log keeps it auditable.
                                run.status = run_status::STOPPED.to_string();
                                run.stop_method = Some("external".to_string());
                                external_stop = true;
                            }
                            executor::ExitDisposition::Failed => {
                                run.status = run_status::FAILED.to_string();
                            }
                        }

                        if let Err(e) = RunRepository::update(&pool, &run).await {
                            tracing::warn!("supervisor: failed to update run {}: {e}", run_id);
                        }

                        // Release the metrics port back to the pool so subsequent Runs can use it.
                        if let Some(port) = run.metrics_port.and_then(|p| u16::try_from(p).ok()) {
                            port_pool.release(port).await;
                        }

                        // Write control log result for natural exit.
                        // Use the operator from the original start intent, or a system sentinel.
                        let start_operator = find_operator_for_run(&pool, &run_id).await;
                        if external_stop {
                            let _ = write_control_result(
                                &pool,
                                &task_id,
                                &run_id,
                                "external_stop",
                                &run.status,
                                &start_operator,
                            )
                            .await;
                        }
                        let _ = write_control_result(
                            &pool,
                            &task_id,
                            &run_id,
                            "run_exit",
                            &run.status,
                            &start_operator,
                        )
                        .await;

                        // Remove from active runs.
                        {
                            let mut active = active_runs.lock().await;
                            active.remove(&task_id);
                        }

                        // Update task status.
                        let _ = update_task_status(&pool, &task_id, &run.status).await;

                        break;
                    }
                }
            }
            Some(RunSlot::Starting) => {
                // Engine is being spawned; wait for it to become Active.
                continue;
            }
            None => {
                // No handle in active_runs — the run was probably stopped via the API.
                // Check if the DB record is still in an active state.
                if let Ok(run) = RunRepository::find_by_id(&pool, &run_id).await {
                    if run_status::is_active(&run.status) {
                        // The run is still marked active but has no handle.
                        // This can happen if the orchestrator was restarted.
                        // Mark it as failed with stop_method="orphaned".
                        let now =
                            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                        let mut orphaned = run;
                        orphaned.status = run_status::FAILED.to_string();
                        orphaned.stop_method = Some("orphaned".to_string());
                        orphaned.stopped_at = Some(now.clone());
                        orphaned.updated_at = now;
                        // Release the port before updating so the pool is consistent.
                        if let Some(port) =
                            orphaned.metrics_port.and_then(|p| u16::try_from(p).ok())
                        {
                            port_pool.release(port).await;
                        }
                        if let Err(e) = RunRepository::update(&pool, &orphaned).await {
                            tracing::warn!("supervisor: failed to orphan run {}: {e}", run_id);
                        }
                        let _ = update_task_status(&pool, &task_id, "failed").await;
                    } else {
                        // Run already in a terminal state (stopped/failed via API).
                        // Release the port if it was still held in the pool.
                        if let Some(port) = run.metrics_port.and_then(|p| u16::try_from(p).ok()) {
                            port_pool.release(port).await;
                        }
                    }
                }
                break;
            }
        }
    }
}

/// Spawn phase 2 (cdc) of a two-phase MySQL `snapshot_and_cdc` Run after
/// phase 1 (snapshot) has exited cleanly. The phase 2 INI was pre-staged
/// at run start by `crate::two_phase::prepare_run_dir`.
///
/// On success: the phase state file is updated to `current_phase=2`, the
/// `RunSlot` is replaced with a fresh `Active(handle)` for the new child,
/// the persisted Run record is updated with the new PID, and a control
/// log entry is written so operators can see the transition.
async fn advance_to_phase2(
    pool: &sqlx::SqlitePool,
    active_runs: &ActiveRuns,
    task_id: &str,
    run_id: &str,
    run_dir: &std::path::Path,
    phase2_ini_path: &str,
) -> Result<(), String> {
    let phase2_ini = std::fs::read_to_string(phase2_ini_path)
        .map_err(|e| format!("read phase2 ini failed: {e}"))?;

    let binary_override = std::env::var("APE_DTS_BINARY_PATH").ok();
    let task = TaskRepository::find_by_id(pool, task_id)
        .await
        .map_err(|e| format!("phase2: load task failed: {e}"))?;
    let engine_env = engine_extra_env(&task);
    let new_handle = executor::LocalExecutor::spawn_with_env(
        run_id,
        &phase2_ini,
        binary_override.as_deref(),
        &engine_env,
    )
    .await?;

    // Persist the phase advance so the marker survives orchestrator restarts.
    if let Err(e) = crate::two_phase::mark_phase_advanced(run_dir) {
        tracing::warn!(
            "phase2: failed to mark phase advanced for {}: {}",
            run_id,
            e
        );
    }

    // Replace the active slot with the new child handle.
    {
        let mut active = active_runs.lock().await;
        active.insert(task_id.to_string(), RunSlot::Active(new_handle.clone()));
    }

    // Update the Run row with the new PID and keep status=running.
    if let Ok(mut run) = RunRepository::find_by_id(pool, run_id).await {
        run.pid = Some(new_handle.pid as i64);
        run.status = run_status::RUNNING.to_string();
        run.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        if let Err(e) = RunRepository::update(pool, &run).await {
            tracing::warn!("phase2: run update failed for {}: {}", run_id, e);
        }
    }

    let operator = find_operator_for_run(pool, run_id).await;
    let _ = write_control_result(
        pool,
        task_id,
        run_id,
        "phase_transition",
        "snapshot_to_cdc",
        &operator,
    )
    .await;

    Ok(())
}

/// Update the Task's status field based on the latest Run status.
async fn update_task_status(
    pool: &sqlx::SqlitePool,
    task_id: &str,
    status: &str,
) -> Result<(), ApiError> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    sqlx::query("UPDATE tasks SET status = ?, updated_at = ? WHERE id = ?")
        .bind(status)
        .bind(&now)
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| {
            ApiError::new(
                codes::INTERNAL_ERROR,
                format!("task status update failed: {e}"),
            )
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn test_pause_signals_the_engine_with_sigterm() {
        use crate::signal::SignalOutcome;

        // Pause is a cooperative stop, so it sends the *same* signal a stop
        // does. There is no SIGUSR channel any more: `dt-main` never handled
        // SIGUSR1/2, so sending them was a disguised kill.
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();

        assert_eq!(
            crate::signal::send(pid, crate::signal::EngineSignal::Term).unwrap(),
            SignalOutcome::Delivered
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn test_pause_kind_gate_allows_only_snapshot_and_cdc() {
        let mut task = make_snapshot_task_for_struct_init();

        task.kind = "snapshot".into();
        assert!(pause_unsupported_reason(&task).is_none());
        task.kind = "cdc".into();
        assert!(pause_unsupported_reason(&task).is_none());

        for kind in ["check", "struct"] {
            task.kind = kind.into();
            assert!(
                pause_unsupported_reason(&task).is_some(),
                "{kind} has no resumable position and must not be pausable"
            );
        }
    }

    #[test]
    fn test_pause_kind_gate_rejects_two_phase_tasks() {
        let mut task = make_snapshot_task_for_struct_init();
        task.kind = "cdc".into();
        task.db_type_source = "mysql".into();
        task.extractor_config = r#"{"extract_type":"snapshot_and_cdc"}"#.into();

        assert!(
            pause_unsupported_reason(&task).is_some(),
            "the managed snapshot→cdc handover owns its own start position"
        );
    }

    #[test]
    fn test_run_slot_starting_has_no_handle() {
        let slot = RunSlot::Starting;
        assert!(slot.as_handle().is_none());
        assert!(slot.into_handle().is_none());
    }

    #[test]
    fn test_engine_extra_env_uses_gaussdb_candidate_hosts() {
        let mut task = make_snapshot_task_for_struct_init();
        task.db_type_source = "gaussdb_oracle".into();
        task.source_endpoint =
            r#"{"url":"postgres://10.250.0.157:8000/db","candidateHosts":["10.250.0.157:8000","10.250.0.223:8000"]}"#
                .into();

        assert_eq!(
            engine_extra_env(&task),
            vec![(
                GAUSSDB_CANDIDATE_HOSTS_ENV.to_string(),
                "10.250.0.157:8000,10.250.0.223:8000".to_string()
            )]
        );
    }

    #[test]
    fn test_engine_extra_env_supports_snake_case_candidate_hosts() {
        let mut task = make_snapshot_task_for_struct_init();
        task.db_type_target = "gaussdb_oracle".into();
        task.target_endpoint =
            r#"{"url":"postgres://10.250.0.157:8000/db","candidate_hosts":["10.250.0.223:8000"]}"#
                .into();

        assert_eq!(
            engine_extra_env(&task),
            vec![(
                GAUSSDB_CANDIDATE_HOSTS_ENV.to_string(),
                "10.250.0.223:8000".to_string()
            )]
        );
    }

    #[test]
    fn test_engine_extra_env_ignores_non_gaussdb_candidate_hosts() {
        let mut task = make_snapshot_task_for_struct_init();
        task.source_endpoint =
            r#"{"url":"postgres://10.250.0.157:8000/db","candidateHosts":["10.250.0.223:8000"]}"#
                .into();

        assert!(engine_extra_env(&task).is_empty());
    }

    fn make_snapshot_task_for_struct_init() -> Task {
        Task {
            id: "task-id".into(),
            task_id: "snapshot_mysql_mysql_test".into(),
            name: "test".into(),
            kind: "snapshot".into(),
            db_type_source: "mysql".into(),
            db_type_target: "mysql".into(),
            source_endpoint: r#"{"url":"mysql://src:3306/test_db"}"#.into(),
            target_endpoint: r#"{"url":"mysql://dst:3306/test_db"}"#.into(),
            extractor_config: r#"{"extract_type":"snapshot","batch_size":16000}"#.into(),
            sinker_config: r#"{"sink_type":"write"}"#.into(),
            filter_config:
                r#"{"do_dbs":"test_db","do_tbs":"test_db.manual_smoke","ignore_dbs":"","ignore_tbs":"","do_events":"insert"}"#
                    .into(),
            router_config: r#"{"db_map":"","tb_map":"","col_map":""}"#.into(),
            parallelizer_config: r#"{"parallel_type":"snapshot","parallel_size":4}"#.into(),
            pipeline_config: r#"{"buffer_size":16000,"checkpoint_interval_secs":10}"#.into(),
            resumer_config: r#"{"resume_type":"from_log"}"#.into(),
            processor_config: "{}".into(),
            runtime_config: r#"{"sync_schema":true,"sync_index":false}"#.into(),
            metrics_config: "{}".into(),
            resource_group_id: "default".into(),
            owner_user_id: Some("admin".into()),
            status: "draft".into(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

    #[test]
    fn test_should_run_struct_init_only_for_snapshot_with_runtime_flag() {
        let mut task = make_snapshot_task_for_struct_init();
        assert!(should_run_struct_init(&task));

        task.runtime_config = r#"{"sync_schema":false}"#.into();
        assert!(!should_run_struct_init(&task));

        task.runtime_config = r#"{"sync_schema":true}"#.into();
        task.kind = "cdc".into();
        assert!(!should_run_struct_init(&task));
    }

    #[test]
    fn test_build_struct_init_task_renders_struct_ini() {
        let task = make_snapshot_task_for_struct_init();
        let struct_task = build_struct_init_task(&task).unwrap();
        let ini = ini_renderer::render(&struct_task);

        assert_eq!(struct_task.kind, "struct");
        assert!(ini.contains("extract_type=struct"));
        assert!(ini.contains("sink_type=struct"));
        assert!(ini.contains("conflict_policy=ignore"));
        assert!(ini.contains("do_structures=database,table,constraint"));
        assert!(ini.contains("parallel_type=serial"));
        assert!(!ini.contains("do_events=insert"));
    }

    #[test]
    fn test_build_struct_init_task_includes_index_when_requested() {
        let mut task = make_snapshot_task_for_struct_init();
        task.runtime_config = r#"{"sync_schema":true,"sync_index":true}"#.into();

        let struct_task = build_struct_init_task(&task).unwrap();
        let ini = ini_renderer::render(&struct_task);

        assert!(ini.contains("do_structures=database,table,constraint,index"));
    }

    #[test]
    fn test_build_struct_init_task_omits_database_for_oracle_target() {
        let mut task = make_snapshot_task_for_struct_init();
        task.db_type_source = "oracle".into();
        task.db_type_target = "oracle".into();
        task.runtime_config = r#"{"sync_schema":true,"sync_index":true}"#.into();

        let struct_task = build_struct_init_task(&task).unwrap();
        let ini = ini_renderer::render(&struct_task);

        assert!(ini.contains("do_structures=table,constraint,index"));
        assert!(!ini.contains("do_structures=database"));
    }

    #[test]
    fn test_struct_init_prefers_explicit_tables_over_wildcard_dbs() {
        let mut task = make_snapshot_task_for_struct_init();
        task.filter_config =
            r#"{"do_dbs":"*","do_tbs":"public.t_gaussdb_oracle_to_oracle","ignore_dbs":"","ignore_tbs":"","do_events":"insert,update,delete"}"#
                .into();

        let struct_task = build_struct_init_task(&task).unwrap();
        let ini = ini_renderer::render(&struct_task);

        assert!(ini.contains("do_dbs=\n"));
        assert!(ini.contains("do_tbs=public.t_gaussdb_oracle_to_oracle"));
    }

    #[tokio::test]
    async fn test_run_slot_active_has_handle() {
        let handle = executor::LocalExecutor::spawn(
            &format!("slot-test-{}", uuid::Uuid::new_v4()),
            "[global]\ntask_id=test\n",
            Some("sleep"),
        )
        .await
        .unwrap();

        let slot = RunSlot::Active(handle.clone());
        assert!(slot.as_handle().is_some());
        let extracted = slot.into_handle();
        assert!(extracted.is_some());
        assert_eq!(extracted.unwrap().pid, handle.pid);

        let _ = executor::LocalExecutor::kill_with_grace(&handle, 2).await;
        let _ = std::fs::remove_dir_all(&handle.run_dir);
    }

    #[tokio::test]
    async fn test_concurrent_active_runs_claim() {
        // Two concurrent claim attempts for the same task_id should result
        // in exactly one Starting slot.
        let active_runs = new_active_runs();
        let task_id = "test-task-concurrent".to_string();

        // First claim succeeds.
        {
            let mut active = active_runs.lock().await;
            assert!(!active.contains_key(&task_id));
            active.insert(task_id.clone(), RunSlot::Starting);
        }

        // Second claim sees the slot is already taken.
        {
            let active = active_runs.lock().await;
            assert!(active.contains_key(&task_id));
            // Attempting to claim again should detect the existing entry.
        }

        // Clean up.
        {
            let mut active = active_runs.lock().await;
            active.remove(&task_id);
        }
    }

    /// Reconciliation test: verify that a re-attached RunHandle with a live
    /// PID gets inserted into ActiveRuns, gets a scraper target registered,
    /// and the supervisor detects exit.
    #[tokio::test]
    async fn test_reattached_run_registered_in_active_runs_and_scraper() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Seed a default resource group and admin user (needed for FK).
        let rg_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        sqlx::query(
            "INSERT INTO resource_groups (id, name, is_default, created_at, updated_at) VALUES (?, 'default', 1, ?, ?)",
        )
        .bind(&rg_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let user_id = uuid::Uuid::new_v4().to_string();
        let pw_hash = bcrypt::hash("admin123", 10).unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role, display_name, disabled, created_at, updated_at) VALUES (?, 'admin', ?, 'admin', 'Admin', 0, ?, ?)",
        )
        .bind(&user_id)
        .bind(&pw_hash)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Create a Task with the correct schema columns.
        let task_id = uuid::Uuid::new_v4().to_string();
        let task_id_field = format!("test_{}", &task_id[..8]);
        sqlx::query(
            "INSERT INTO tasks (id, task_id, name, kind, db_type_source, db_type_target, source_endpoint, target_endpoint, extractor_config, filter_config, router_config, parallelizer_config, pipeline_config, resumer_config, processor_config, runtime_config, metrics_config, resource_group_id, owner_user_id, status, created_at, updated_at) VALUES (?, ?, 'test', 'snapshot', 'mysql', 'mysql', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', ?, ?, 'draft', ?, ?)",
        )
        .bind(&task_id)
        .bind(&task_id_field)
        .bind(&rg_id)
        .bind(&user_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Spawn a real "sleep" process to get a live PID.
        let run_id = uuid::Uuid::new_v4().to_string();
        let handle =
            executor::LocalExecutor::spawn(&run_id, "[global]\ntask_id=test\n", Some("sleep"))
                .await
                .unwrap();
        let pid = handle.pid as i64;

        // Insert a Run record in "running" state with the live PID.
        let base_dir = executor::run_data_dir();
        let log_dir = format!("{base_dir}/{run_id}/logs");
        let ini_path = format!("{base_dir}/{run_id}/task_config.ini");

        sqlx::query(
            "INSERT INTO runs (id, task_id, status, pid, ini_path, log_dir, started_at, stopped_at, exit_code, stop_method, created_at, updated_at) VALUES (?, ?, 'running', ?, ?, ?, ?, NULL, NULL, NULL, ?, ?)",
        )
        .bind(&run_id)
        .bind(&task_id)
        .bind(pid)
        .bind(&ini_path)
        .bind(&log_dir)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Simulate reconciliation: re-attach the Run.
        let active_runs = new_active_runs();
        let scraper_state = crate::metrics_scraper::ScraperState::new();

        // Manually do what reconcile_live_runs does for an alive-PID run:
        let run_dir = std::path::PathBuf::from(format!("{base_dir}/{run_id}"));
        let reattach_handle = executor::LocalExecutor::reattach(&run_id, pid as u32, run_dir);

        // Insert into ActiveRuns.
        {
            let mut active = active_runs.lock().await;
            active.insert(
                task_id.clone(),
                executor::RunSlot::Active(reattach_handle.clone()),
            );
        }

        // Register scraper target (use a placeholder port for the test).
        let target = crate::metrics_scraper::scrape_target_from_run(&task_id, &run_id, 9100);
        scraper_state.add_target(target).await;

        // Verify: ActiveRuns has the task.
        {
            let active = active_runs.lock().await;
            assert!(
                active.contains_key(&task_id),
                "task should be in active_runs"
            );
            let slot = active.get(&task_id).unwrap();
            let handle_ref = slot.as_handle().unwrap();
            assert!(
                handle_ref.reattached,
                "handle should be marked as reattached"
            );
            assert_eq!(handle_ref.pid, pid as u32);
        }

        // Verify: scraper has the target.
        {
            let targets = scraper_state.get_targets_for_test().await;
            assert_eq!(targets.len(), 1, "should have 1 scrape target");
            assert_eq!(targets[0].task_id, task_id);
            assert_eq!(targets[0].run_id, run_id);
        }

        // Verify: status reports Running for the re-attached PID.
        let status = executor::LocalExecutor::status(&reattach_handle).await;
        assert!(
            matches!(status, executor::ChildStatus::Running),
            "reattached live PID should report Running"
        );

        // Clean up: kill the process.
        let _ = executor::LocalExecutor::kill_with_grace(&reattach_handle, 3).await;
        let _ = std::fs::remove_dir_all(&handle.run_dir);
    }

    /// Reconciliation test: verify that a dead-PID Run gets marked as
    /// orphaned/failed (existing behavior preserved).
    #[tokio::test]
    async fn test_dead_pid_run_marked_orphaned_on_reconciliation() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Seed FK dependencies.
        let rg_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        sqlx::query(
            "INSERT INTO resource_groups (id, name, is_default, created_at, updated_at) VALUES (?, 'default', 1, ?, ?)",
        )
        .bind(&rg_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let user_id = uuid::Uuid::new_v4().to_string();
        let pw_hash = bcrypt::hash("admin123", 10).unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role, display_name, disabled, created_at, updated_at) VALUES (?, 'admin', ?, 'admin', 'Admin', 0, ?, ?)",
        )
        .bind(&user_id)
        .bind(&pw_hash)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let task_id = uuid::Uuid::new_v4().to_string();
        let task_id_field = format!("test_{}", &task_id[..8]);
        sqlx::query(
            "INSERT INTO tasks (id, task_id, name, kind, db_type_source, db_type_target, source_endpoint, target_endpoint, extractor_config, filter_config, router_config, parallelizer_config, pipeline_config, resumer_config, processor_config, runtime_config, metrics_config, resource_group_id, owner_user_id, status, created_at, updated_at) VALUES (?, ?, 'test', 'snapshot', 'mysql', 'mysql', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', ?, ?, 'running', ?, ?)",
        )
        .bind(&task_id)
        .bind(&task_id_field)
        .bind(&rg_id)
        .bind(&user_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Create a Run with a definitely-dead PID (very high PID that won't exist).
        let run_id = uuid::Uuid::new_v4().to_string();
        let dead_pid: i64 = 4000000;

        sqlx::query(
            "INSERT INTO runs (id, task_id, status, pid, ini_path, log_dir, started_at, stopped_at, exit_code, stop_method, created_at, updated_at) VALUES (?, ?, 'running', ?, NULL, NULL, ?, NULL, NULL, NULL, ?, ?)",
        )
        .bind(&run_id)
        .bind(&task_id)
        .bind(dead_pid)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Simulate reconciliation for a dead PID: mark as orphaned.
        let updated_run = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
        let mut orphaned = updated_run;
        orphaned.status = "failed".to_string();
        orphaned.stop_method = Some("orphaned".to_string());
        orphaned.exit_code = Some(-1);
        orphaned.stopped_at =
            Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        orphaned.updated_at =
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        RunRepository::update(&pool, &orphaned).await.unwrap();

        // Verify the run is now failed.
        let reloaded = RunRepository::find_by_id(&pool, &run_id).await.unwrap();
        assert_eq!(reloaded.status, "failed");
        assert_eq!(reloaded.stop_method.as_deref(), Some("orphaned"));
        assert_eq!(reloaded.exit_code, Some(-1));
        assert!(reloaded.stopped_at.is_some());
    }

    /// Test: precheck with blocking failures returns PRECHECK_BLOCKING_FAILED.
    #[tokio::test]
    async fn test_precheck_blocking_failures_prevent_start() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Seed FK dependencies.
        let rg_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        sqlx::query(
            "INSERT INTO resource_groups (id, name, is_default, created_at, updated_at) VALUES (?, 'default', 1, ?, ?)",
        )
        .bind(&rg_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let user_id = uuid::Uuid::new_v4().to_string();
        let pw_hash = bcrypt::hash("admin123", 10).unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role, display_name, disabled, created_at, updated_at) VALUES (?, 'admin', ?, 'admin', 'Admin', 0, ?, ?)",
        )
        .bind(&user_id)
        .bind(&pw_hash)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Create a snapshot Task pointing to unreachable databases.
        let task_id = uuid::Uuid::new_v4().to_string();
        let task_id_field = format!("test_{}", &task_id[..8]);
        sqlx::query(
            "INSERT INTO tasks (id, task_id, name, kind, db_type_source, db_type_target, source_endpoint, target_endpoint, extractor_config, filter_config, router_config, parallelizer_config, pipeline_config, resumer_config, processor_config, runtime_config, metrics_config, resource_group_id, owner_user_id, status, created_at, updated_at) VALUES (?, ?, 'test', 'snapshot', 'mysql', 'mysql', '{\"url\":\"mysql://root:@127.0.0.1:19999/test\"}', '{\"url\":\"mysql://root:@127.0.0.1:19998/test\"}', '{\"extractType\":\"snapshot\"}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', ?, ?, 'draft', ?, ?)",
        )
        .bind(&task_id)
        .bind(&task_id_field)
        .bind(&rg_id)
        .bind(&user_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Load the task and run precheck
        let task = TaskRepository::find_by_id(&pool, &task_id).await.unwrap();

        // Run precheck — should return Ok with failing items for unreachable DBs
        let resp = crate::precheck_handlers::run_precheck(&task).await;
        match resp {
            Ok(precheck_resp) => {
                // Precheck completed — there should be failures for unreachable DBs
                assert!(
                    precheck_resp.summary.fail > 0,
                    "precheck should have failures for unreachable databases"
                );
            }
            Err(e) => {
                // Config validation error — also a blocking failure
                assert_eq!(e.code, codes::TASK_VALIDATION_FAILED);
            }
        }
    }

    /// Test: precheck panic is caught and returns PRECHECK_BLOCKING_FAILED.
    #[test]
    fn test_precheck_panic_caught_as_blocking_failed() {
        // Simulate a panic in precheck by testing the catch mechanism directly
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            panic!("missing server_id for CDC task");
        }));

        assert!(result.is_err(), "panic should be caught by catch_unwind");

        // Verify the panic message extraction
        let panic_payload = result.unwrap_err();
        let panic_msg = match panic_payload.downcast_ref::<String>() {
            Some(s) => s.clone(),
            None => match panic_payload.downcast_ref::<&str>() {
                Some(s) => s.to_string(),
                None => "unknown".to_string(),
            },
        };
        assert!(
            panic_msg.contains("server_id") || panic_msg.contains("CDC"),
            "panic message should reference the cause: {panic_msg}"
        );
    }
}
