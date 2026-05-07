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

use crate::error::{codes, ApiError};
use crate::ini_renderer;
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{CreateTaskRequest, Task, UserContext};
use crate::repositories::task_repository::TaskRepository;

use dt_common::config::task_config::TaskConfig;
use dt_precheck::builder::prechecker_builder::PrecheckerBuilder;
use dt_precheck::config::precheck_config::PrecheckConfig;
use dt_precheck::config::task_config::PrecheckTaskConfig;
use dt_precheck::meta::check_result::CheckResult;
use dt_precheck::prechecker::traits::Prechecker;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestConnectionResponse {
    pub source: ConnectionSideResult,
    pub target: ConnectionSideResult,
}

/// A single precheck item in the response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecheckItem {
    pub name: String,
    pub side: String,
    pub status: String, // "pass" | "fail" | "skip"
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
/// section suitable for the given task kind.
fn write_temp_ini(task: &Task, kind: &str) -> Result<PathBuf, ApiError> {
    let ini_text = ini_renderer::render(task);
    // Append [precheck] section required by PrecheckTaskConfig
    let do_cdc = kind == "cdc";
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
async fn test_one_side(builder: &PrecheckerBuilder, is_source: bool) -> ConnectionSideResult {
    match builder.build_checker(is_source) {
        Some(mut checker) => match checker.build_connection().await {
            Ok(result) => {
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
            Err(e) => ConnectionSideResult {
                ok: false,
                code: Some("CONNECTION_FAILED".to_string()),
                message: Some(e.to_string()),
            },
        },
        None => ConnectionSideResult {
            ok: false,
            code: Some("UNSUPPORTED_ENGINE".to_string()),
            message: Some("no checker available for this engine type".to_string()),
        },
    }
}

/// Convert a CheckResult to a PrecheckItem.
fn check_result_to_item(result: &CheckResult, is_source: bool) -> PrecheckItem {
    let side = if is_source { "source" } else { "target" };
    let status = if result.is_validate { "pass" } else { "fail" };
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
    let db_type_source =
        crate::validation::resolve_db_type(&body.engine_source, body.sub_mode.as_deref());
    let db_type_target =
        crate::validation::resolve_db_type(&body.engine_target, body.sub_mode.as_deref());

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

// ─── Handlers ────────────────────────────────────────────────────────────

/// POST /api/tasks/:id/test_connection — test connectivity for a persisted task.
#[post("/tasks/{id}/test_connection")]
pub async fn test_connection(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let task_id = path.into_inner();
    let task = match TaskRepository::find_by_id(&pool, &task_id).await {
        Ok(t) => t,
        Err(_) => return ApiError::new(codes::TASK_NOT_FOUND, "Task not found").error_response(),
    };

    do_test_connection(&task).await
}

/// POST /api/tasks/preview/test_connection — draft mode (no persistence).
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

    do_test_connection(&task).await
}

/// POST /api/tasks/:id/precheck — run prerequisite checks for a persisted task.
#[post("/tasks/{id}/precheck")]
pub async fn precheck(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let task_id = path.into_inner();
    let task = match TaskRepository::find_by_id(&pool, &task_id).await {
        Ok(t) => t,
        Err(_) => return ApiError::new(codes::TASK_NOT_FOUND, "Task not found").error_response(),
    };

    do_precheck(&task).await
}

/// POST /api/tasks/preview/precheck — draft mode (no persistence).
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

    do_precheck(&task).await
}

// ─── Core logic (shared between persisted and draft modes) ──────────────

/// Core test_connection logic. Tests source and target connectivity independently.
async fn do_test_connection(task: &Task) -> HttpResponse {
    let ini_path = match write_temp_ini(task, &task.kind) {
        Ok(p) => p,
        Err(e) => return e.error_response(),
    };

    let result = {
        let (task_config, precheck_config) = match load_configs(&ini_path) {
            Ok(c) => c,
            Err(e) => {
                cleanup_temp_ini(&ini_path);
                return e.error_response();
            }
        };

        let builder = PrecheckerBuilder::build(precheck_config, task_config);

        if !builder.valid_config() {
            cleanup_temp_ini(&ini_path);
            return ApiError::new(
                codes::TASK_VALIDATION_FAILED,
                "invalid config: source or target URL is empty",
            )
            .error_response();
        }

        // Test source and target independently — one failure does not abort the other
        let source = test_one_side(&builder, true).await;
        let target = test_one_side(&builder, false).await;

        HttpResponse::Ok().json(TestConnectionResponse { source, target })
    };

    cleanup_temp_ini(&ini_path);
    result
}

/// Core precheck logic. Runs all applicable checks and returns per-item results.
/// A single failing check does NOT panic the orchestrator.
async fn do_precheck(task: &Task) -> HttpResponse {
    // Struct kind: return empty-but-OK
    if task.kind == "struct" {
        return HttpResponse::Ok().json(PrecheckResponse {
            items: vec![],
            summary: PrecheckSummary {
                pass: 0,
                fail: 0,
                skip: 0,
            },
        });
    }

    let ini_path = match write_temp_ini(task, &task.kind) {
        Ok(p) => p,
        Err(e) => return e.error_response(),
    };

    let result = {
        let (task_config, precheck_config) = match load_configs(&ini_path) {
            Ok(c) => c,
            Err(e) => {
                cleanup_temp_ini(&ini_path);
                return e.error_response();
            }
        };

        let builder = PrecheckerBuilder::build(precheck_config, task_config);
        let do_cdc = task.kind == "cdc";

        if !builder.valid_config() {
            cleanup_temp_ini(&ini_path);
            return ApiError::new(
                codes::TASK_VALIDATION_FAILED,
                "invalid config: source or target URL is empty",
            )
            .error_response();
        }

        let mut items = Vec::new();

        // Run source checks
        if let Some(mut source_checker) = builder.build_checker(true) {
            items.extend(run_side_checks(&mut source_checker, true, do_cdc, &task.kind).await);
        }

        // Run target checks
        if let Some(mut sink_checker) = builder.build_checker(false) {
            items.extend(run_side_checks(&mut sink_checker, false, do_cdc, &task.kind).await);
        }

        // Compute summary
        let mut pass = 0;
        let mut fail = 0;
        let mut skip = 0;
        for item in &items {
            match item.status.as_str() {
                "pass" => pass += 1,
                "fail" => fail += 1,
                _ => skip += 1,
            }
        }

        HttpResponse::Ok().json(PrecheckResponse {
            items,
            summary: PrecheckSummary { pass, fail, skip },
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
    use crate::middleware::csrf::{XSRF_COOKIE_NAME, XSRF_HEADER_NAME};
    use crate::rate_limit::RateLimiter;
    use crate::run_handlers::new_active_runs;
    use actix_web::cookie::{Cookie, Key};
    use actix_web::test as actix_test;
    use actix_web::App;

    const XSRF: &str = "test-xsrf-token";

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
        crate::build_app(
            Key::from(&master_bytes),
            pool_clone,
            rate_limiter,
            3600,
            active_runs,
            scraper_state,
            log_sse_state,
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

        let resp = do_test_connection(&task).await;
        // Should return 200 even with both sides failing (VAL-CONN-004)
        assert_eq!(resp.status(), 200);

        let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
        let conn_resp: TestConnectionResponse = serde_json::from_slice(&body).unwrap();
        assert!(!conn_resp.source.ok, "source should fail");
        assert!(!conn_resp.target.ok, "target should fail");
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

        let resp = do_test_connection(&task).await;
        assert_eq!(resp.status(), 200);

        let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
        let conn_resp: TestConnectionResponse = serde_json::from_slice(&body).unwrap();
        // Each side reports independently — one failure does NOT abort the other
        assert!(!conn_resp.source.ok);
        assert!(!conn_resp.target.ok);
        assert!(conn_resp.source.code.is_some());
        assert!(conn_resp.target.code.is_some());
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
        ];
        let mut pass = 0;
        let mut fail = 0;
        let mut skip = 0;
        for item in &items {
            match item.status.as_str() {
                "pass" => pass += 1,
                "fail" => fail += 1,
                _ => skip += 1,
            }
        }
        assert_eq!(pass, 1);
        assert_eq!(fail, 1);
        assert_eq!(skip, 1);
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
            builder.build_checker(true).is_some(),
            "mysql source checker"
        );
        assert!(
            builder.build_checker(false).is_some(),
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
        assert!(builder.build_checker(true).is_some(), "pg source checker");
        assert!(builder.build_checker(false).is_some(), "pg target checker");
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

        let resp = do_test_connection(&task).await;
        // Empty URL → 422 validation error
        assert_eq!(resp.status(), 422);
    }
}
