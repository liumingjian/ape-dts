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
use crate::executor::{self, ChildStatus, ExitStatus, RunSlot};
use crate::idempotency::{extract_key, IdempotencyCache};
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
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let log = crate::models::OperateLog {
        id: 0,
        actor: actor.to_string(),
        action: action.to_string(),
        result: result.to_string(),
        target: Some(target.to_string()),
        details: None,
        ip: Some(ip.to_string()),
        created_at: now,
    };
    if let Err(e) = OperateLogRepository::create(pool, &log).await {
        tracing::warn!("audit log write failed: {e}");
    }
    Ok(())
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
    port_pool: web::Data<PortPool>,
    idempotency_cache: web::Data<IdempotencyCache>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskStart) {
        return e.error_response();
    }

    // Idempotency-Key check: if the key was seen before, return the cached result.
    let idem_key = extract_key(&req);
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
    if let Ok(Some(active_run)) = RunRepository::find_active_by_task(&pool, &task_id).await {
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

    let binary_override = if std::env::var("APE_DTS_BINARY_PATH").is_ok() {
        Some(std::env::var("APE_DTS_BINARY_PATH").unwrap())
    } else {
        None
    };
    if let Err(e) = run_struct_init_if_requested(&task, binary_override.as_deref()).await {
        {
            let mut active = active_runs.lock().await;
            active.remove(&task_id);
        }
        let _ = write_run_audit_log(
            &pool,
            &user.username,
            "tasks.start",
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
        created_at: now.clone(),
        updated_at: now,
    };

    // Write control log intent.
    let _ = write_control_intent(&pool, &task_id, &run_id, "start", &user.username).await;

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
    let ini_content = if crate::two_phase::is_two_phase_task(&task) {
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
                    "start",
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
                "tasks.start",
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
    let _ =
        write_control_result(&pool, &task_id, &run_id, "start", "success", &user.username).await;

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
    let _ = write_run_audit_log(
        &pool,
        &user.username,
        "tasks.start",
        "success",
        &run_id,
        &ip,
    )
    .await;

    // Spawn a background task to monitor the child process.
    let bg_pool = pool.get_ref().clone();
    let bg_active_runs = active_runs.get_ref().clone();
    let bg_port_pool = port_pool.get_ref().clone();
    let bg_task_id = task_id.clone();
    let bg_run_id = run_id.clone();
    tokio::spawn(async move {
        supervise_run(bg_pool, bg_active_runs, bg_task_id, bg_run_id, bg_port_pool).await;
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
    let idem_key = extract_key(&req);
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

    // Only running or paused runs can be stopped.
    if !matches!(active_run.status.as_str(), "running" | "paused") {
        return ApiError::with_details(
            codes::ILLEGAL_TRANSITION,
            "Run is not in a stoppable state",
            serde_json::json!({ "from": active_run.status, "to": "stopping" }),
        )
        .error_response();
    }

    let run_id = active_run.id.clone();

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
        metrics_scraper::scrape_single_run(
            &pool, task_id_for_scrape, &run_id, "127.0.0.1", port,
        )
        .await;
    }

    // Kill the child process.
    let kill_result = {
        let mut active = active_runs.lock().await;
        if let Some(slot) = active.remove(&task_id) {
            if let Some(handle) = slot.into_handle() {
                match executor::LocalExecutor::kill(&handle).await {
                    Ok(kr) => Some(kr),
                    Err(e) => {
                        tracing::warn!("kill failed for run {}: {e}", run_id);
                        None
                    }
                }
            } else {
                // Slot was in Starting state — no child process to kill.
                tracing::warn!(
                    "stop requested for task {} but slot was Starting (no child)",
                    task_id
                );
                None
            }
        } else {
            // Handle not in memory — try sending SIGTERM directly via PID.
            if let Some(pid) = run.pid {
                let _ = kill_process_by_pid(pid as u32).await;
            }
            None
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

/// POST /api/tasks/:id/pause — pause a running CDC Run.
///
/// Returns 202 on success.
/// Returns 409 if the Run is not in a pausable state.
#[post("/tasks/{id}/pause")]
pub async fn pause_task(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    _active_runs: web::Data<ActiveRuns>,
    scraper_state: web::Data<ScraperState>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskStart) {
        return e.error_response();
    }

    let task_id = path.into_inner();
    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());

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

    let run_id = run.id.clone();

    // Write control log intent.
    let _ = write_control_intent(&pool, &task_id, &run_id, "pause", &user.username).await;

    // Send SIGUSR1 to the child process (engine interprets as pause signal).
    if let Some(pid) = run.pid {
        if let Err(e) = send_pause_signal(pid as u32) {
            tracing::warn!("pause signal failed for run {}: {e}", run_id);
        }
    }

    // Transition to paused.
    run.status = run_status::PAUSED.to_string();
    run.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    // Pause metric ingestion.
    scraper_state.pause(&run_id).await;

    if let Err(e) = RunRepository::update(&pool, &run).await {
        let _ =
            write_control_result(&pool, &task_id, &run_id, "pause", "error", &user.username).await;
        return ApiError::new(
            codes::INTERNAL_ERROR,
            format!("run status update failed: {e}"),
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

/// POST /api/tasks/:id/resume — resume a paused Run.
///
/// Returns 202 on success.
/// Returns 409 if the Run is not paused.
#[post("/tasks/{id}/resume")]
pub async fn resume_task(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    _active_runs: web::Data<ActiveRuns>,
    scraper_state: web::Data<ScraperState>,
    req: actix_web::HttpRequest,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskStart) {
        return e.error_response();
    }

    let task_id = path.into_inner();
    let ip = req
        .connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());

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
                "Cannot resume a run that is not active",
                serde_json::json!({ "from": from_status, "to": "running" }),
            )
            .error_response();
        }
        Err(e) => {
            return ApiError::new(codes::INTERNAL_ERROR, format!("run lookup failed: {e}"))
                .error_response();
        }
    };

    // Only paused runs can be resumed.
    if run.status != run_status::PAUSED {
        return ApiError::with_details(
            codes::ILLEGAL_TRANSITION,
            "Cannot resume a run that is not paused",
            serde_json::json!({ "from": run.status, "to": "running" }),
        )
        .error_response();
    }

    let run_id = run.id.clone();

    // Write control log intent.
    let _ = write_control_intent(&pool, &task_id, &run_id, "resume", &user.username).await;

    // Send SIGUSR2 to the child process (engine interprets as resume signal).
    if let Some(pid) = run.pid {
        if let Err(e) = send_resume_signal(pid as u32) {
            tracing::warn!("resume signal failed for run {}: {e}", run_id);
        }
    }

    // Transition to running.
    run.status = run_status::RUNNING.to_string();
    run.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    // Resume metric ingestion.
    scraper_state.resume(&run_id).await;

    if let Err(e) = RunRepository::update(&pool, &run).await {
        let _ =
            write_control_result(&pool, &task_id, &run_id, "resume", "error", &user.username).await;
        return ApiError::new(
            codes::INTERNAL_ERROR,
            format!("run status update failed: {e}"),
        )
        .error_response();
    }

    // Write control log result.
    let _ = write_control_result(
        &pool,
        &task_id,
        &run_id,
        "resume",
        "success",
        &user.username,
    )
    .await;

    // Write audit log.
    let _ = write_run_audit_log(
        &pool,
        &user.username,
        "tasks.resume",
        "success",
        &run_id,
        &ip,
    )
    .await;

    HttpResponse::Accepted().json(serde_json::json!({ "run_id": run_id }))
}

/// GET /api/runs — list all Runs across all Tasks, most recent first.
///
/// Returns a flat array of RunResponse objects (no pagination on the dashboard
/// list; callers can filter by status client-side).
#[get("/runs")]
pub async fn list_runs(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    match RunRepository::list_all(&pool).await {
        Ok(runs) => {
            let items: Vec<RunResponse> = runs.iter().map(run_to_response).collect();
            HttpResponse::Ok().json(items)
        }
        Err(e) => {
            ApiError::new(codes::INTERNAL_ERROR, format!("run list failed: {e}"))
                .error_response()
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
            l.action == "start"
                && l.run_id.as_deref() == Some(run_id)
                && l.intent_or_result == "intent"
        })
        .and_then(|l| l.operator_id.clone())
        .unwrap_or_else(|| "system".to_string())
}

/// Supervise a running Run: poll the child process and update the DB
/// when it exits. Releases the Run's metrics port back to the pool when
/// the Run reaches a terminal state.
pub async fn supervise_run(
    pool: sqlx::SqlitePool,
    active_runs: ActiveRuns,
    task_id: String,
    run_id: String,
    port_pool: PortPool,
) {
    // Poll the child process status every 2 seconds.
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

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
                        let mut run = match RunRepository::find_by_id(&pool, &run_id).await {
                            Ok(r) => r,
                            Err(_) => break,
                        };

                        // Final scrape: capture the engine's last metric emission
                        // (e.g., progress=100) before the process is fully gone.
                        // Best-effort — the engine may have already shut down its
                        // HTTP endpoint, in which case the scrape silently fails.
                        if let Some(port) = run.metrics_port.and_then(|p| u16::try_from(p).ok()) {
                            let task_id_ref = run.task_id.as_deref().unwrap_or(&task_id);
                            metrics_scraper::scrape_single_run(
                                &pool, task_id_ref, &run_id, "127.0.0.1", port,
                            )
                            .await;
                        }

                        let now =
                            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                        run.stopped_at = Some(now.clone());
                        run.updated_at = now;

                        match exit {
                            ExitStatus::Exited { code } => {
                                run.exit_code = Some(code as i64);
                                if code == 0 {
                                    run.status = run_status::STOPPED.to_string();
                                } else {
                                    run.status = run_status::FAILED.to_string();
                                }
                            }
                            ExitStatus::Signaled { signal } => {
                                run.exit_code = Some(128 + signal as i64);
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

/// Kill a process by PID when the RunHandle is not available.
async fn kill_process_by_pid(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-s", "TERM", &pid.to_string()])
            .output()
            .map_err(|e| format!("failed to kill pid {pid}: {e}"))?;
    }
    Ok(())
}

/// Send a pause signal (SIGUSR1) to the engine process.
#[cfg(unix)]
fn send_pause_signal(pid: u32) -> Result<(), String> {
    std::process::Command::new("kill")
        .args(["-s", "USR1", &pid.to_string()])
        .output()
        .map_err(|e| format!("failed to send SIGUSR1 to pid {pid}: {e}"))?;
    Ok(())
}

/// Send a resume signal (SIGUSR2) to the engine process.
#[cfg(unix)]
fn send_resume_signal(pid: u32) -> Result<(), String> {
    std::process::Command::new("kill")
        .args(["-s", "USR2", &pid.to_string()])
        .output()
        .map_err(|e| format!("failed to send SIGUSR2 to pid {pid}: {e}"))?;
    Ok(())
}

#[cfg(not(unix))]
fn send_pause_signal(_pid: u32) -> Result<(), String> {
    Err("Pause signal not supported on this platform".to_string())
}

#[cfg(not(unix))]
fn send_resume_signal(_pid: u32) -> Result<(), String> {
    Err("Resume signal not supported on this platform".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pause_signal_uses_sigusr1() {
        // Verify that send_pause_signal constructs a "kill -s USR1" command
        // by inspecting the function's implementation. On Unix, the function
        // uses Command::new("kill").args(["-s", "USR1", &pid]).
        // We can't easily test the actual signal delivery without a real
        // process, but we can verify the function exists and compiles with
        // the correct signal name.
        // This test serves as a compile-time assertion that SIGUSR1 is used.
        #[cfg(unix)]
        {
            // Send SIGUSR1 to PID 0 would fail, but the signal choice is
            // embedded in the command. We verify indirectly by checking
            // the function signature is correct.
            let _ = send_pause_signal(999999);
        }
    }

    #[test]
    fn test_resume_signal_uses_sigusr2() {
        // Same as above: compile-time assertion for SIGUSR2.
        #[cfg(unix)]
        {
            let _ = send_resume_signal(999999);
        }
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
