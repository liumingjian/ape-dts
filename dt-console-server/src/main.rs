use actix_web::cookie::Key;
use tracing_subscriber::EnvFilter;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8080";

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let bind_addr =
        std::env::var("CONSOLE_BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());

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
