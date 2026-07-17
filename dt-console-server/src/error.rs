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
    pub const TASK_VALIDATION_FAILED: &str = "TASK_VALIDATION_FAILED";

    // Auth
    pub const UNAUTHENTICATED: &str = "UNAUTHENTICATED";
    pub const INVALID_CREDENTIALS: &str = "INVALID_CREDENTIALS";
    pub const ACCOUNT_DISABLED: &str = "ACCOUNT_DISABLED";
    pub const SESSION_EXPIRED: &str = "SESSION_EXPIRED";
    pub const FORBIDDEN: &str = "FORBIDDEN";

    // CSRF
    pub const CSRF_TOKEN_MISSING: &str = "CSRF_TOKEN_MISSING";
    pub const CSRF_TOKEN_MISMATCH: &str = "CSRF_TOKEN_MISMATCH";

    // Rate limit
    pub const TOO_MANY_ATTEMPTS: &str = "TOO_MANY_ATTEMPTS";

    // Resource
    pub const NOT_FOUND: &str = "NOT_FOUND";
    pub const CONFLICT: &str = "CONFLICT";
    pub const USERNAME_TAKEN: &str = "USERNAME_TAKEN";
    pub const LAST_ADMIN_PROTECTED: &str = "LAST_ADMIN_PROTECTED";
    pub const CANNOT_DEMOTE_SELF: &str = "CANNOT_DEMOTE_SELF";

    // License
    pub const LICENSE_LIMIT_EXCEEDED: &str = "LICENSE_LIMIT_EXCEEDED";
    pub const LICENSE_EXPIRED: &str = "LICENSE_EXPIRED";
    pub const INVALID_LICENSE_CODE: &str = "INVALID_LICENSE_CODE";

    // Task
    pub const TASK_NOT_FOUND: &str = "TASK_NOT_FOUND";
    pub const TASK_KIND_IMMUTABLE: &str = "TASK_KIND_IMMUTABLE";
    pub const TASK_HAS_ACTIVE_RUN: &str = "TASK_HAS_ACTIVE_RUN";

    // Run
    pub const RUN_ALREADY_ACTIVE: &str = "RUN_ALREADY_ACTIVE";
    pub const RUN_NOT_ACTIVE: &str = "RUN_NOT_ACTIVE";
    pub const ILLEGAL_TRANSITION: &str = "ILLEGAL_TRANSITION";
    pub const RUN_NOT_FOUND: &str = "RUN_NOT_FOUND";

    // Task validation
    pub const GAUSSDB_SUB_MODE_REQUIRED: &str = "GAUSSDB_SUB_MODE_REQUIRED";
    pub const UNKNOWN_GAUSSDB_SUB_MODE: &str = "UNKNOWN_GAUSSDB_SUB_MODE";
    pub const SYNC_MODE_INVALID_FOR_CATEGORY: &str = "SYNC_MODE_INVALID_FOR_CATEGORY";
    pub const STRUCT_FILTER_REQUIRED: &str = "STRUCT_FILTER_REQUIRED";
    pub const INVALID_URL_SCHEME: &str = "INVALID_URL_SCHEME";
    pub const URL_SCHEME_ENGINE_MISMATCH: &str = "URL_SCHEME_ENGINE_MISMATCH";
    pub const PATH_OUTSIDE_SANDBOX: &str = "PATH_OUTSIDE_SANDBOX";
    pub const ENDPOINT_HOST_BLOCKED: &str = "ENDPOINT_HOST_BLOCKED";
    pub const UNKNOWN_RESOURCE_GROUP: &str = "UNKNOWN_RESOURCE_GROUP";
    pub const PRECHECK_BLOCKING_FAILED: &str = "PRECHECK_BLOCKING_FAILED";
    pub const STRUCT_INIT_FAILED: &str = "STRUCT_INIT_FAILED";
    /// The precheck task itself crashed (panic or unrecoverable internal
    /// error). Carries `details.requestId` for log correlation.
    pub const PRECHECK_PANIC: &str = "PRECHECK_PANIC";

    /// A test_connection probe exceeded the bounded connect deadline.
    /// Surfaces as a per-side `code` so a hung TCP connect cannot drop the
    /// HTTP socket. Carries the timeout value in the human-readable message.
    pub const TEST_CONNECTION_TIMEOUT: &str = "TEST_CONNECTION_TIMEOUT";

    /// The test_connection task itself crashed (panic or unrecoverable
    /// internal error). Carries `details.requestId` for log correlation —
    /// matches `PRECHECK_PANIC`'s contract.
    pub const TEST_CONNECTION_PANIC: &str = "TEST_CONNECTION_PANIC";

    /// A side's filter config (do_dbs / do_tbs / ignore_dbs / ignore_tbs)
    /// failed to parse. Surfaces as a per-side `code` on test_connection
    /// so a malformed filter does NOT panic the underlying builder.
    pub const INVALID_FILTER_CONFIG: &str = "INVALID_FILTER_CONFIG";

    // Resource Group
    pub const RESOURCE_GROUP_NAME_TAKEN: &str = "RESOURCE_GROUP_NAME_TAKEN";
    pub const DEFAULT_RESOURCE_GROUP_PROTECTED: &str = "DEFAULT_RESOURCE_GROUP_PROTECTED";
    pub const RESOURCE_GROUP_HAS_TASKS: &str = "RESOURCE_GROUP_HAS_TASKS";
    pub const RESOURCE_GROUP_NOT_FOUND: &str = "RESOURCE_GROUP_NOT_FOUND";

    // Export
    pub const UNSUPPORTED_EXPORT_FORMAT: &str = "UNSUPPORTED_EXPORT_FORMAT";

    // Log / SSE
    pub const UNKNOWN_LOG_FILE: &str = "UNKNOWN_LOG_FILE";
    pub const REPLAY_GAP: &str = "REPLAY_GAP";

    // Schema / migration
    pub const SCHEMA_MISMATCH: &str = "SCHEMA_MISMATCH";

    /// All 100 per-Run metrics ports in [9100, 9199] are currently allocated.
    /// No new Run can be started until an existing one reaches a terminal state.
    pub const PORT_POOL_EXHAUSTED: &str = "PORT_POOL_EXHAUSTED";

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

            codes::NOT_FOUND
            | codes::TASK_NOT_FOUND
            | codes::RUN_NOT_FOUND
            | codes::RESOURCE_GROUP_NOT_FOUND
            | codes::UNKNOWN_LOG_FILE => StatusCode::NOT_FOUND,

            codes::PARSE_ERROR | codes::VALIDATION_FAILED => StatusCode::BAD_REQUEST,

            codes::CANNOT_DEMOTE_SELF
            | codes::LICENSE_LIMIT_EXCEEDED
            | codes::GAUSSDB_SUB_MODE_REQUIRED
            | codes::UNKNOWN_GAUSSDB_SUB_MODE
            | codes::SYNC_MODE_INVALID_FOR_CATEGORY
            | codes::STRUCT_FILTER_REQUIRED
            | codes::INVALID_URL_SCHEME
            | codes::URL_SCHEME_ENGINE_MISMATCH
            | codes::PATH_OUTSIDE_SANDBOX
            | codes::ENDPOINT_HOST_BLOCKED
            | codes::UNKNOWN_RESOURCE_GROUP
            | codes::TASK_KIND_IMMUTABLE
            | codes::TASK_VALIDATION_FAILED
            | codes::PRECHECK_BLOCKING_FAILED
            | codes::STRUCT_INIT_FAILED
            | codes::PRECHECK_PANIC
            | codes::TEST_CONNECTION_PANIC => StatusCode::UNPROCESSABLE_ENTITY,

            codes::TEST_CONNECTION_TIMEOUT => StatusCode::GATEWAY_TIMEOUT,

            codes::CONFLICT
            | codes::LAST_ADMIN_PROTECTED
            | codes::USERNAME_TAKEN
            | codes::TASK_HAS_ACTIVE_RUN
            | codes::RUN_ALREADY_ACTIVE
            | codes::RUN_NOT_ACTIVE
            | codes::ILLEGAL_TRANSITION
            | codes::RESOURCE_GROUP_NAME_TAKEN
            | codes::DEFAULT_RESOURCE_GROUP_PROTECTED
            | codes::RESOURCE_GROUP_HAS_TASKS
            | codes::PORT_POOL_EXHAUSTED => StatusCode::CONFLICT,

            codes::LICENSE_EXPIRED => StatusCode::UNPROCESSABLE_ENTITY,

            codes::INVALID_LICENSE_CODE | codes::UNSUPPORTED_EXPORT_FORMAT => {
                StatusCode::BAD_REQUEST
            }

            codes::TOO_MANY_ATTEMPTS => StatusCode::TOO_MANY_REQUESTS,

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
