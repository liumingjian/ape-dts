use actix_web::cookie::Key;
use dt_console_server::db::{self, DbError, SCHEMA_MISMATCH_CODE};
use tracing_subscriber::EnvFilter;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8080";
const DEFAULT_DB_PATH: &str = "./data/console.db";

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let bind_addr =
        std::env::var("CONSOLE_BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());

    // Initialise the SQLite database: create pool, run migrations, verify schema.
    let db_path = std::env::var("CONSOLE_DB_PATH").unwrap_or_else(|_| DEFAULT_DB_PATH.to_string());

    let _pool = match db::init(&db_path).await {
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

    // Generate the session encryption key once; clone the master bytes for each
    // worker thread so sessions are valid across all workers.
    let key = Key::generate();
    let master_bytes = key.master().to_vec();

    tracing::info!("dt-console-server starting on {bind_addr}");

    actix_web::HttpServer::new(move || {
        let key = Key::from(&master_bytes);
        dt_console_server::build_app(key)
    })
    .bind(&bind_addr)?
    .run()
    .await
}
