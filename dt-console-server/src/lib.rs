pub mod error;
pub mod health;
pub mod middleware;

use actix_cors::Cors;
use actix_session::storage::CookieSessionStore;
use actix_session::SessionMiddleware;
use actix_web::cookie::Key;
use actix_web::web::{self, JsonConfig};
use actix_web::App;

use middleware::csrf::Csrf;

/// Configure all API routes.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/api").service(health::healthz));
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
        .configure(configure)
}
