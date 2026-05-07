use actix_web::cookie::Key;
use dt_console_server::alarm_dispatcher;
use dt_console_server::alert_engine;
use dt_console_server::alert_handlers;
use dt_console_server::auth;
use dt_console_server::db::{self, DbError, SCHEMA_MISMATCH_CODE};
use dt_console_server::idempotency::IdempotencyCache;
use dt_console_server::log_sse_handlers;
use dt_console_server::metrics_scraper;
use dt_console_server::models::{ResourceGroup, Run};
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use dt_console_server::repositories::resource_group_repository::ResourceGroupRepository;
use dt_console_server::repositories::run_repository::RunRepository;
use dt_console_server::run_handlers;
use dt_console_server::time_series_store;
use tracing_subscriber::EnvFilter;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8080";
const DEFAULT_DB_PATH: &str = "./data/console.db";
const DEFAULT_IDLE_TIMEOUT_SECS: i64 = 3600;
const DEFAULT_SCRAPE_INTERVAL_SECS: u64 = 10;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let bind_addr =
        std::env::var("CONSOLE_BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());

    // Initialise the SQLite database: create pool, run migrations, verify schema.
    let db_path = std::env::var("CONSOLE_DB_PATH").unwrap_or_else(|_| DEFAULT_DB_PATH.to_string());

    let pool = match db::init(&db_path).await {
        Ok(pool) => pool,
        Err(DbError::SchemaMismatch(msg)) => {
            tracing::error!(code = SCHEMA_MISMATCH_CODE, "{msg}");
            std::process::exit(1);
        }
        Err(e) => {
            tracing::error!("database initialisation failed: {e}");
            std::process::exit(1);
        }
    };

    // Seed the default admin user if the users table is empty.
    if let Err(e) = auth::seed_admin(&pool).await {
        tracing::error!("admin user seeding failed: {e}");
        std::process::exit(1);
    }

    // Seed the default resource group if the resource_groups table is empty.
    if let Err(e) = seed_default_resource_group(&pool).await {
        tracing::error!("default resource group seeding failed: {e}");
        std::process::exit(1);
    }

    // Finalise orphaned control-log intents from a previous orchestrator session.
    dt_console_server::control_log_handlers::finalise_orphaned_intents(&pool).await;

    // Read idle timeout from env, or use default.
    let idle_timeout_secs = std::env::var("CONSOLE_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS);

    // Create the rate limiter.
    let rate_limiter = RateLimiter::new(RateLimitConfig::default());

    // Create the active runs registry.
    let active_runs = run_handlers::new_active_runs();

    // Create the scraper state for metrics scraping.
    let scraper_state = metrics_scraper::ScraperState::new();

    // Create the SSE state for log streaming.
    let log_sse_state = log_sse_handlers::LogSseState::default();

    // Reconcile live Runs from a previous orchestrator session.
    // Must run AFTER active_runs and scraper_state are created so that
    // re-attached Runs can be registered for supervision and scraping.
    reconcile_live_runs(&pool, &active_runs, &scraper_state).await;

    // Create the alert SSE state for alert streaming.
    let alert_sse_state = alert_handlers::AlertSseState::new();

    // Create the alarm dispatcher state.
    let dispatcher_state = alarm_dispatcher::DispatcherState::new();

    // Create the alert engine state.
    let alert_engine_state = alert_engine::AlertEngineState::new();

    // Create the idempotency cache for lifecycle/clear dedup.
    let idempotency_cache = IdempotencyCache::new();

    // Read scrape interval from env, or use default.
    let scrape_interval_secs = std::env::var("CONSOLE_SCRAPE_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SCRAPE_INTERVAL_SECS);

    // Spawn the background scraper loop.
    metrics_scraper::spawn_scraper(pool.clone(), scraper_state.clone(), scrape_interval_secs);

    // Spawn the background retention sweep loop.
    time_series_store::spawn_retention_loop(pool.clone());

    // Generate the session encryption key once; clone the master bytes for each
    // worker thread so sessions are valid across all workers.
    let key = Key::generate();
    let master_bytes = key.master().to_vec();

    tracing::info!("dt-console-server starting on {bind_addr}");

    // Set up graceful SIGTERM handling.
    // Spawn a task that marks the scraper as not running on shutdown,
    // so the readyz endpoint reflects degraded state during shutdown.
    let shutdown_scraper = scraper_state.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("received shutdown signal, marking scraper as stopped");
        shutdown_scraper.set_running(false).await;
    });

    let server = actix_web::HttpServer::new(move || {
        let key = Key::from(&master_bytes);
        let pool_clone = pool.clone();
        let rate_limiter_clone = rate_limiter.clone();
        let active_runs_clone = active_runs.clone();
        let scraper_state_clone = scraper_state.clone();
        let log_sse_state_clone = log_sse_state.clone();
        let alert_sse_state_clone = alert_sse_state.clone();
        let dispatcher_state_clone = dispatcher_state.clone();
        let alert_engine_state_clone = alert_engine_state.clone();
        dt_console_server::build_app(
            key,
            pool_clone,
            rate_limiter_clone,
            idle_timeout_secs,
            active_runs_clone,
            scraper_state_clone,
            log_sse_state_clone,
            alert_sse_state_clone,
            dispatcher_state_clone,
            alert_engine_state_clone,
            idempotency_cache.clone(),
        )
    })
    .bind(&bind_addr)?;

    // Graceful SIGTERM: stop accepting new requests, finish in-flight, exit.
    // actix-web handles SIGTERM and ctrl+c internally via the run() method:
    // it stops accepting new connections and waits for in-flight requests
    // to complete (up to the graceful shutdown timeout), then exits cleanly.
    server.run().await
}

/// Reconcile live Runs from a previous orchestrator session.
///
/// On restart, any Run in a non-terminal state (pending, running, paused,
/// stopping) must be reconciled:
/// - If the PID is still alive, re-attach:
///   - Reconstruct a `RunHandle` (with `reattached = true`)
///   - Insert into the `ActiveRuns` registry
///   - Spawn a `supervise_run` background task
///   - Register as a scrape target for the MetricsScraper
/// - If the PID is dead or missing, mark the Run as failed with
///   stop_method="orphaned" and exit_status populated.
///
/// Log tailers are reconnected lazily when the first SSE subscriber connects,
/// so no explicit reconnection is needed here.
async fn reconcile_live_runs(
    pool: &sqlx::SqlitePool,
    active_runs: &run_handlers::ActiveRuns,
    scraper_state: &metrics_scraper::ScraperState,
) {
    let active_statuses = ["pending", "running", "paused", "stopping"];

    let runs: Vec<Run> = match RunRepository::list_by_statuses(pool, &active_statuses).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("failed to list active runs for reconciliation: {e}");
            return;
        }
    };

    if runs.is_empty() {
        return;
    }

    tracing::info!(
        "reconciling {} active runs from previous session",
        runs.len()
    );

    for run in runs {
        let pid_alive = match run.pid {
            Some(pid) if pid > 0 => {
                // Check if the process is still alive.
                // On Unix, sending signal 0 to a PID checks existence without affecting it.
                #[cfg(unix)]
                {
                    unsafe { libc::kill(pid as i32, 0) == 0 }
                }
                #[cfg(not(unix))]
                {
                    false
                }
            }
            _ => false,
        };

        if pid_alive {
            let pid = run.pid.unwrap() as u32;

            tracing::info!(run_id = %run.id, pid = pid, "re-attaching to live run");

            // Derive run_dir from the Run's log_dir or ini_path.
            let run_dir = run
                .log_dir
                .as_ref()
                .or(run.ini_path.as_ref())
                .map(|p| {
                    std::path::PathBuf::from(p)
                        .parent()
                        .map(|parent| parent.to_path_buf())
                        .unwrap_or_default()
                })
                .unwrap_or_else(|| {
                    let base = run_handlers::executor_run_data_dir();
                    std::path::PathBuf::from(format!("{base}/{}", run.id))
                });

            // Reconstruct a RunHandle for the live process.
            let handle =
                dt_console_server::executor::LocalExecutor::reattach(&run.id, pid, run_dir);

            // Insert into the ActiveRuns registry.
            {
                let mut active = active_runs.lock().await;
                active.insert(
                    run.task_id.clone(),
                    dt_console_server::executor::RunSlot::Active(handle.clone()),
                );
            }

            // Register as a scrape target.
            {
                let target = metrics_scraper::scrape_target_from_run(&run.task_id, &run.id);
                scraper_state.add_target(target).await;
            }

            // Spawn a background supervise_run task.
            let bg_pool = pool.clone();
            let bg_active_runs = active_runs.clone();
            let bg_task_id = run.task_id.clone();
            let bg_run_id = run.id.clone();
            tokio::spawn(async move {
                run_handlers::supervise_run(bg_pool, bg_active_runs, bg_task_id, bg_run_id).await;
            });
        } else {
            tracing::info!(run_id = %run.id, "marking orphaned run as failed");
            let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let mut updated = run;
            updated.status = "failed".to_string();
            updated.stop_method = Some("orphaned".to_string());
            updated.stopped_at = Some(now);
            if updated.exit_code.is_none() {
                updated.exit_code = Some(-1);
            }
            updated.updated_at =
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

            if let Err(e) = RunRepository::update(pool, &updated).await {
                tracing::warn!(run_id = %updated.id, "failed to mark orphaned run: {e}");
            }
        }
    }
}

/// Seed the default resource group if none exist.
async fn seed_default_resource_group(pool: &sqlx::SqlitePool) -> Result<(), String> {
    let existing = ResourceGroupRepository::list(pool)
        .await
        .map_err(|e| format!("resource group list failed: {e}"))?;

    if !existing.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let rg = ResourceGroup {
        id: uuid::Uuid::new_v4().to_string(),
        name: "default".to_string(),
        is_default: true,
        created_at: now.clone(),
        updated_at: now,
    };

    ResourceGroupRepository::create(pool, &rg)
        .await
        .map_err(|e| format!("default resource group seed failed: {e}"))?;

    tracing::info!("seeded default resource group");
    Ok(())
}
