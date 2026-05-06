//! Error envelope and custom extractors for the API.
//!
//! All non-2xx responses use the standard envelope shape:
//! ```json
//! { "code": "STRING_CODE", "message": "human-readable", "details": <optional> }
//! ```

use actix_web::{http::StatusCode, HttpResponse};
use serde::Serialize;

/// Well-known error codes used throughout the API.
pub mod codes {
    // Parse / validation
    pub const PARSE_ERROR: &str = "PARSE_ERROR";
    pub const VALIDATION_FAILED: &str = "VALIDATION_FAILED";

    // Auth
    pub const UNAUTHENTICATED: &str = "UNAUTHENTICATED";
    pub const INVALID_CREDENTIALS: &str = "INVALID_CREDENTIALS";
    pub const ACCOUNT_DISABLED: &str = "ACCOUNT_DISABLED";
    pub const SESSION_EXPIRED: &str = "SESSION_EXPIRED";
    pub const FORBIDDEN: &str = "FORBIDDEN";

    // CSRF
    pub const CSRF_TOKEN_MISSING: &str = "CSRF_TOKEN_MISSING";
    pub const CSRF_TOKEN_MISMATCH: &str = "CSRF_TOKEN_MISMATCH";

    // Resource
    pub const NOT_FOUND: &str = "NOT_FOUND";
    pub const CONFLICT: &str = "CONFLICT";

    // License
    pub const LICENSE_LIMIT_EXCEEDED: &str = "LICENSE_LIMIT_EXCEEDED";
    pub const LICENSE_EXPIRED: &str = "LICENSE_EXPIRED";
    pub const INVALID_LICENSE_CODE: &str = "INVALID_LICENSE_CODE";

    // Schema / migration
    pub const SCHEMA_MISMATCH: &str = "schema_mismatch";

    // Internal
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
}

/// Standard error envelope for all non-2xx responses.
#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: Some(details),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl actix_web::ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self.code.as_str() {
            codes::UNAUTHENTICATED
            | codes::INVALID_CREDENTIALS
            | codes::ACCOUNT_DISABLED
            | codes::SESSION_EXPIRED => StatusCode::UNAUTHORIZED,

            codes::FORBIDDEN | codes::CSRF_TOKEN_MISSING | codes::CSRF_TOKEN_MISMATCH => {
                StatusCode::FORBIDDEN
            }

            codes::NOT_FOUND => StatusCode::NOT_FOUND,

            codes::PARSE_ERROR | codes::VALIDATION_FAILED => StatusCode::BAD_REQUEST,

            codes::CONFLICT | codes::LICENSE_LIMIT_EXCEEDED => StatusCode::CONFLICT,

            codes::LICENSE_EXPIRED | codes::INVALID_LICENSE_CODE => StatusCode::BAD_REQUEST,

            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(self)
    }
}

/// Helper to build an error response with the standard envelope.
pub fn error_response(
    status: StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
    details: Option<serde_json::Value>,
) -> HttpResponse {
    let err = match details {
        Some(d) => ApiError::with_details(code, message, d),
        None => ApiError::new(code, message),
    };
    HttpResponse::build(status).json(err)
}
