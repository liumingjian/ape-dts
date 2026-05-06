use actix_web::cookie::Key;
use dt_console_server::auth;
use dt_console_server::db::{self, DbError, SCHEMA_MISMATCH_CODE};
use dt_console_server::models::ResourceGroup;
use dt_console_server::rate_limit::{RateLimitConfig, RateLimiter};
use dt_console_server::repositories::resource_group_repository::ResourceGroupRepository;
use tracing_subscriber::EnvFilter;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8080";
const DEFAULT_DB_PATH: &str = "./data/console.db";
const DEFAULT_IDLE_TIMEOUT_SECS: i64 = 3600;

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

    // Read idle timeout from env, or use default.
    let idle_timeout_secs = std::env::var("CONSOLE_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS);

    // Create the rate limiter.
    let rate_limiter = RateLimiter::new(RateLimitConfig::default());

    // Generate the session encryption key once; clone the master bytes for each
    // worker thread so sessions are valid across all workers.
    let key = Key::generate();
    let master_bytes = key.master().to_vec();

    tracing::info!("dt-console-server starting on {bind_addr}");

    actix_web::HttpServer::new(move || {
        let key = Key::from(&master_bytes);
        let pool_clone = pool.clone();
        let rate_limiter_clone = rate_limiter.clone();
        dt_console_server::build_app(key, pool_clone, rate_limiter_clone, idle_timeout_secs)
    })
    .bind(&bind_addr)?
    .run()
    .await
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
