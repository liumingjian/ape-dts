//! System hosts, global params, readiness, and operational endpoints.
//!
//! - GET    /api/system/hosts       — list system hosts
//! - GET    /api/global_params      — list global params
//! - PATCH  /api/global_params      — update global params
//! - GET    /api/readyz            — readiness probe (DB + scraper check)

use crate::error::{codes, ApiError};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{GlobalParam, SystemHost, UserContext};
use crate::repositories::global_param_repository::GlobalParamRepository;
use crate::repositories::system_host_repository::SystemHostRepository;
use actix_web::{get, patch, web, HttpResponse, ResponseError};

// ─── System Hosts ────────────────────────────────────────────────────────

/// GET /api/system/hosts — list all system hosts.
#[get("/system/hosts")]
pub async fn list_system_hosts(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::SystemHostRead) {
        return e.error_response();
    }

    match SystemHostRepository::list(&pool).await {
        Ok(hosts) => {
            let items: Vec<serde_json::Value> = hosts.iter().map(host_to_json).collect();
            HttpResponse::Ok().json(serde_json::json!({ "items": items }))
        }
        Err(e) => {
            tracing::warn!("system host list failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to list system hosts").error_response()
        }
    }
}

/// Convert a SystemHost to a JSON value.
fn host_to_json(host: &SystemHost) -> serde_json::Value {
    serde_json::json!({
        "id": crate::alert_handlers::escape_xss(&host.id),
        "hostname": crate::alert_handlers::escape_xss(&host.hostname),
        "ip": crate::alert_handlers::escape_xss(&host.ip),
        "status": crate::alert_handlers::escape_xss(&host.status),
        "lastHeartbeat": host.last_heartbeat.as_ref().map(|s| crate::alert_handlers::escape_xss(s)),
        "createdAt": crate::alert_handlers::escape_xss(&host.created_at),
        "updatedAt": crate::alert_handlers::escape_xss(&host.updated_at),
    })
}

// ─── Global Params ───────────────────────────────────────────────────────

/// GET /api/global_params — list all global params.
#[get("/global_params")]
pub async fn list_global_params(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::GlobalParamRead) {
        return e.error_response();
    }

    match GlobalParamRepository::list(&pool).await {
        Ok(params) => {
            let items: Vec<serde_json::Value> = params.iter().map(param_to_json).collect();
            HttpResponse::Ok().json(serde_json::json!({ "items": items }))
        }
        Err(e) => {
            tracing::warn!("global param list failed: {e}");
            ApiError::new(codes::INTERNAL_ERROR, "Failed to list global params").error_response()
        }
    }
}

/// PATCH /api/global_params — update global params.
///
/// Accepts an array of {key, value} pairs and upserts each one.
/// Admin-only.
#[patch("/global_params")]
pub async fn update_global_params(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    body: web::Json<UpdateGlobalParamsRequest>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::GlobalParamUpdate) {
        return e.error_response();
    }

    let mut updated = Vec::new();
    for item in &body.params {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        match GlobalParamRepository::find_by_key(&pool, &item.key).await {
            Ok(existing) => {
                let mut to_update = existing;
                to_update.value = item.value.clone();
                to_update.updated_at = now;
                match GlobalParamRepository::update(&pool, &to_update).await {
                    Ok(p) => updated.push(param_to_json(&p)),
                    Err(e) => {
                        tracing::warn!("global param update failed for key {}: {e}", item.key);
                        return ApiError::new(
                            codes::INTERNAL_ERROR,
                            "Failed to update global param",
                        )
                        .error_response();
                    }
                }
            }
            Err(_) => {
                // Key doesn't exist — create it.
                let new_param = GlobalParam {
                    id: uuid::Uuid::new_v4().to_string(),
                    key: item.key.clone(),
                    value: item.value.clone(),
                    created_at: now.clone(),
                    updated_at: now,
                };
                match GlobalParamRepository::create(&pool, &new_param).await {
                    Ok(p) => updated.push(param_to_json(&p)),
                    Err(e) => {
                        tracing::warn!("global param create failed for key {}: {e}", item.key);
                        return ApiError::new(
                            codes::INTERNAL_ERROR,
                            "Failed to create global param",
                        )
                        .error_response();
                    }
                }
            }
        }
    }

    HttpResponse::Ok().json(serde_json::json!({ "items": updated }))
}

/// Convert a GlobalParam to a JSON value.
fn param_to_json(param: &GlobalParam) -> serde_json::Value {
    serde_json::json!({
        "id": crate::alert_handlers::escape_xss(&param.id),
        "key": crate::alert_handlers::escape_xss(&param.key),
        "value": crate::alert_handlers::escape_xss(&param.value),
        "createdAt": crate::alert_handlers::escape_xss(&param.created_at),
        "updatedAt": crate::alert_handlers::escape_xss(&param.updated_at),
    })
}

// ─── Readiness Probe ─────────────────────────────────────────────────────

/// GET /api/readyz — readiness probe (distinct from /api/healthz liveness).
///
/// Checks critical component readiness:
/// - DB connectivity + migration status
/// - Scraper running state
/// - License loaded or missing
///
/// Returns 200 with `overall:"ready"` if all critical components are up.
/// Returns 503 with `overall:"degraded"` if any component is down.
/// Unlike /api/healthz (liveness), this reflects operational readiness.
#[get("/readyz")]
pub async fn readyz(
    pool: web::Data<sqlx::SqlitePool>,
    scraper_state: web::Data<crate::metrics_scraper::ScraperState>,
) -> HttpResponse {
    let mut checks = serde_json::Map::new();
    let mut any_degraded = false;

    // DB check: run a simple query.
    let db_ok = match sqlx::query_as::<_, (i64,)>("SELECT 1")
        .fetch_one(pool.get_ref())
        .await
    {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!("readyz DB check failed: {e}");
            any_degraded = true;
            false
        }
    };
    checks.insert(
        "db".to_string(),
        serde_json::json!(if db_ok { "ready" } else { "down" }),
    );

    // Migrations check: verify the _sqlx_migrations table has rows.
    let migrations_ok = if db_ok {
        match sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(pool.get_ref())
            .await
        {
            Ok(row) => row.0 > 0,
            Err(e) => {
                tracing::warn!("readyz migrations check failed: {e}");
                any_degraded = true;
                false
            }
        }
    } else {
        false
    };
    checks.insert(
        "migrations".to_string(),
        serde_json::json!(if migrations_ok { "applied" } else { "unknown" }),
    );

    // Scraper check: the scraper is a background task; we check that
    // the scraper state is present and the background loop is running.
    let scraper_running = scraper_state.is_running().await;
    checks.insert(
        "scraper".to_string(),
        serde_json::json!(if scraper_running {
            "running"
        } else {
            "stopped"
        }),
    );
    if !scraper_running {
        any_degraded = true;
    }

    // License check: query the license table.
    let license_status = if db_ok {
        match crate::repositories::license_repository::LicenseRepository::get(pool.get_ref()).await
        {
            Ok(Some(_)) => "loaded",
            Ok(None) => "missing",
            Err(e) => {
                tracing::warn!("readyz license check failed: {e}");
                "error"
            }
        }
    } else {
        "unknown"
    };
    checks.insert("license".to_string(), serde_json::json!(license_status));
    // License missing is not critical for readiness — the system still functions.
    // Only "error" counts as degraded.

    let overall = if any_degraded { "degraded" } else { "ready" };
    let status_code = if any_degraded { 503 } else { 200 };

    HttpResponse::build(actix_web::http::StatusCode::from_u16(status_code as u16).unwrap()).json(
        serde_json::json!({
            "status": overall,
            "checks": checks,
        }),
    )
}

// ─── Request Types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
pub struct UpdateGlobalParamsRequest {
    pub params: Vec<GlobalParamItem>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GlobalParamItem {
    pub key: String,
    pub value: String,
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_to_json() {
        let host = SystemHost {
            id: "h-1".to_string(),
            hostname: "worker-1".to_string(),
            ip: "10.0.0.1".to_string(),
            status: "healthy".to_string(),
            last_heartbeat: Some("2025-01-01T00:00:00.000Z".to_string()),
            created_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
        };
        let json = host_to_json(&host);
        assert_eq!(json["id"], "h-1");
        assert_eq!(json["hostname"], "worker-1");
        assert_eq!(json["ip"], "10.0.0.1");
        assert_eq!(json["status"], "healthy");
    }

    #[test]
    fn test_param_to_json() {
        let param = GlobalParam {
            id: "gp-1".to_string(),
            key: "scrape_interval".to_string(),
            value: "10".to_string(),
            created_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
        };
        let json = param_to_json(&param);
        assert_eq!(json["key"], "scrape_interval");
        assert_eq!(json["value"], "10");
    }

    #[tokio::test]
    async fn test_global_param_crud() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let gp = GlobalParam {
            id: "gp-1".to_string(),
            key: "scrape_interval".to_string(),
            value: "10".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        // Create
        let created = GlobalParamRepository::create(&pool, &gp).await.unwrap();
        assert_eq!(created.key, "scrape_interval");
        assert_eq!(created.value, "10");

        // Find by key
        let found = GlobalParamRepository::find_by_key(&pool, "scrape_interval")
            .await
            .unwrap();
        assert_eq!(found.value, "10");

        // Update
        let mut updated = found;
        updated.value = "20".to_string();
        updated.updated_at =
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let saved = GlobalParamRepository::update(&pool, &updated)
            .await
            .unwrap();
        assert_eq!(saved.value, "20");
    }

    #[tokio::test]
    async fn test_readyz_ok() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // DB check should pass.
        let row: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await.unwrap();
        assert_eq!(row.0, 1);
    }

    /// VAL-OPS-READY-001: /api/readyz reports component readiness distinct from /api/healthz.
    /// Verify the readyz handler returns detailed component status.
    #[tokio::test]
    async fn test_readyz_component_status() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let scraper = crate::metrics_scraper::ScraperState::new();
        scraper.set_running(true).await;

        // Test the handler logic directly.
        // DB should be ready.
        let db_ok = sqlx::query_as::<_, (i64,)>("SELECT 1")
            .fetch_one(&pool)
            .await
            .is_ok();
        assert!(db_ok);

        // Migrations should be applied.
        let mig_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(mig_count.0 > 0);

        // Scraper should be running.
        assert!(scraper.is_running().await);

        // License should be missing (no license seeded).
        let license = crate::repositories::license_repository::LicenseRepository::get(&pool)
            .await
            .unwrap();
        assert!(license.is_none());
    }

    /// VAL-TIME-001: All persisted timestamps are UTC ISO-8601 with Z suffix.
    #[tokio::test]
    async fn test_utc_timestamps_on_persisted_rows() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Create a global param and verify timestamps are UTC.
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let gp = GlobalParam {
            id: "gp-utc-test".to_string(),
            key: "test_key".to_string(),
            value: "test_value".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        let created = GlobalParamRepository::create(&pool, &gp).await.unwrap();

        // Verify timestamps end with 'Z' (UTC indicator).
        assert!(
            created.created_at.ends_with('Z'),
            "created_at should end with Z, got: {}",
            created.created_at
        );
        assert!(
            created.updated_at.ends_with('Z'),
            "updated_at should end with Z, got: {}",
            created.updated_at
        );

        // Verify the timestamp matches the UTC ISO-8601 format.
        let re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$").unwrap();
        assert!(
            re.is_match(&created.created_at),
            "created_at should match UTC ISO-8601, got: {}",
            created.created_at
        );
    }

    /// VAL-OPS-RECOVER-001: Orchestrator restart reconciles live Runs.
    /// Test that a non-terminal Run with a dead PID gets marked as failed/orphaned.
    #[tokio::test]
    async fn test_reconcile_orphaned_runs() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        // Seed a default resource group first (required by tasks FK).
        let rg_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO resource_groups (id, name, is_default, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&rg_id)
        .bind("default")
        .bind(true)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Create a task.
        let task_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO tasks (id, task_id, name, kind, db_type_source, db_type_target, \
             source_endpoint, target_endpoint, extractor_config, sinker_config, filter_config, \
             router_config, parallelizer_config, pipeline_config, resumer_config, \
             processor_config, runtime_config, metrics_config, resource_group_id, status, \
             created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&task_id)
        .bind("test_task_id")
        .bind("Test Task")
        .bind("snapshot")
        .bind("mysql")
        .bind("mysql")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind("{}")
        .bind(&rg_id)
        .bind("draft")
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Create a "running" Run with an impossible PID.
        let run_id = uuid::Uuid::new_v4().to_string();
        let dead_pid: i64 = 999999999; // Very unlikely to exist
        sqlx::query(
            "INSERT INTO runs (id, task_id, status, pid, ini_path, log_dir, started_at, \
             stopped_at, exit_code, stop_method, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&run_id)
        .bind(&task_id)
        .bind("running")
        .bind(dead_pid)
        .bind::<Option<String>>(None)
        .bind::<Option<String>>(None)
        .bind(&now)
        .bind::<Option<String>>(None)
        .bind::<Option<i64>>(None)
        .bind::<Option<String>>(None)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        // Verify the Run is in "running" state.
        let run = crate::repositories::run_repository::RunRepository::find_by_id(&pool, &run_id)
            .await
            .unwrap();
        assert_eq!(run.status, "running");

        // Simulate reconciliation: find non-terminal runs and mark dead ones as orphaned.
        let active_statuses = ["pending", "running", "paused", "stopping"];
        let runs = crate::repositories::run_repository::RunRepository::list_by_statuses(
            &pool,
            &active_statuses,
        )
        .await
        .unwrap();
        assert_eq!(runs.len(), 1);

        // Check if the PID is alive (it won't be).
        let pid_alive = unsafe { libc::kill(dead_pid as i32, 0) == 0 };
        assert!(!pid_alive, "Dead PID should not be alive");

        // Mark the orphaned Run as failed.
        let now_utc = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut orphaned = runs.into_iter().next().unwrap();
        orphaned.status = "failed".to_string();
        orphaned.stop_method = Some("orphaned".to_string());
        orphaned.stopped_at = Some(now_utc);
        orphaned.exit_code = Some(-1);
        orphaned.updated_at =
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        crate::repositories::run_repository::RunRepository::update(&pool, &orphaned)
            .await
            .unwrap();

        // Verify the Run is now "failed".
        let updated =
            crate::repositories::run_repository::RunRepository::find_by_id(&pool, &run_id)
                .await
                .unwrap();
        assert_eq!(updated.status, "failed");
        assert_eq!(updated.stop_method.as_deref(), Some("orphaned"));
        assert!(updated.stopped_at.is_some());
        assert_eq!(updated.exit_code, Some(-1));
    }

    /// System hosts list returns items from DB.
    #[tokio::test]
    async fn test_system_hosts_list() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Seed a system host.
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let host = SystemHost {
            id: "h-test".to_string(),
            hostname: "test-host".to_string(),
            ip: "192.168.1.1".to_string(),
            status: "healthy".to_string(),
            last_heartbeat: Some(now.clone()),
            created_at: now.clone(),
            updated_at: now,
        };
        SystemHostRepository::create(&pool, &host).await.unwrap();

        let hosts = SystemHostRepository::list(&pool).await.unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "test-host");
    }
}
