use actix_web::{middleware::Logger, App, HttpServer};
use tracing_subscriber::EnvFilter;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8080";

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let bind_addr =
        std::env::var("CONSOLE_BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());

    tracing::info!("dt-console-server starting on {bind_addr}");

    HttpServer::new(|| {
        App::new()
            .wrap(Logger::default())
            .configure(dt_console_server::configure)
    })
    .bind(&bind_addr)?
    .run()
    .await
}
