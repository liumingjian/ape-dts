pub mod health;

use actix_web::{web, HttpResponse};

/// Standard error envelope for all non-2xx responses.
#[derive(Debug, serde::Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Helper to build an error response with the standard envelope.
pub fn error_response(
    status: actix_web::http::StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
    details: Option<serde_json::Value>,
) -> HttpResponse {
    HttpResponse::build(status).json(ApiError {
        code: code.into(),
        message: message.into(),
        details,
    })
}

/// Configure all API routes.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/api").service(health::healthz));
}
