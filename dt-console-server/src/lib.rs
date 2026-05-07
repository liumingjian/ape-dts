pub mod auth;
pub mod auth_handlers;
pub mod control_log_handlers;
pub mod db;
pub mod error;
pub mod executor;
pub mod health;
pub mod ini_renderer;
pub mod license_handlers;
pub mod middleware;
pub mod models;
pub mod operate_log_handlers;
pub mod rate_limit;
pub mod repositories;
pub mod resource_group_handlers;
pub mod run_handlers;
pub mod task_handlers;
pub mod user_handlers;
pub mod validation;

use actix_cors::Cors;
use actix_session::storage::CookieSessionStore;
use actix_session::SessionMiddleware;
use actix_web::cookie::Key;
use actix_web::web::{self, JsonConfig};
use actix_web::App;

use middleware::csrf::Csrf;
use rate_limit::RateLimiter;
use run_handlers::ActiveRuns;

/// Configure all API routes.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .service(health::healthz)
            .service(auth_handlers::login)
            .service(auth_handlers::logout)
            .service(auth_handlers::me)
            .service(user_handlers::list_users)
            .service(user_handlers::create_user)
            .service(user_handlers::get_user)
            .service(user_handlers::update_user)
            .service(user_handlers::delete_user)
            .service(task_handlers::create_task)
            .service(task_handlers::list_tasks)
            .service(task_handlers::get_task)
            .service(task_handlers::update_task)
            .service(task_handlers::delete_task)
            .service(task_handlers::preview_ini)
            .service(task_handlers::export_task)
            .service(task_handlers::import_tasks)
            .service(task_handlers::clone_task)
            .service(resource_group_handlers::list_resource_groups)
            .service(resource_group_handlers::create_resource_group)
            .service(resource_group_handlers::get_resource_group)
            .service(resource_group_handlers::update_resource_group)
            .service(resource_group_handlers::delete_resource_group)
            .service(operate_log_handlers::list_operate_logs)
            .service(license_handlers::get_license)
            .service(license_handlers::activate_license)
            // Run lifecycle
            .service(run_handlers::start_task)
            .service(run_handlers::stop_task)
            .service(run_handlers::pause_task)
            .service(run_handlers::resume_task)
            .service(run_handlers::get_run)
            // Control logs
            .service(control_log_handlers::list_control_logs),
    );
}

/// Build the complete actix-web App with all HTTP scaffolding middleware wired.
///
/// This is the canonical app factory used by both the server and integration
/// tests. It layers:
///
/// 1. `Logger` — request logging (outermost)
/// 2. `Cors` — same-origin CORS policy
/// 3. `SessionMiddleware` — cookie-backed sessions via actix-session
/// 4. `Csrf` — XSRF-TOKEN cookie ↔ X-XSRF-TOKEN header enforcement
/// 5. `JsonConfig` — parse errors mapped to `{ code: "PARSE_ERROR" }` envelope
pub fn build_app(
    key: Key,
    pool: sqlx::SqlitePool,
    rate_limiter: RateLimiter,
    idle_timeout_secs: i64,
    active_runs: ActiveRuns,
) -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    let session_mw = SessionMiddleware::builder(CookieSessionStore::default(), key)
        .cookie_name("session".to_string())
        .cookie_secure(false) // dev mode; true behind HTTPS in production
        .cookie_http_only(true)
        .cookie_same_site(actix_web::cookie::SameSite::Lax)
        .cookie_path("/".to_string())
        .build();

    App::new()
        .wrap(actix_web::middleware::Logger::default())
        .wrap(Cors::default())
        .wrap(session_mw)
        .wrap(Csrf)
        .app_data(JsonConfig::default().error_handler(|err, _req| {
            error::ApiError::new(error::codes::PARSE_ERROR, err.to_string()).into()
        }))
        .app_data(web::Data::new(pool))
        .app_data(web::Data::new(rate_limiter))
        .app_data(web::Data::new(idle_timeout_secs))
        .app_data(web::Data::new(active_runs))
        .configure(configure)
}
