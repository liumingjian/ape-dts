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
    pub const CANNOT_DEMOTE_SELF: &str = "cannot_demote_self";

    // License
    pub const LICENSE_LIMIT_EXCEEDED: &str = "LICENSE_LIMIT_EXCEEDED";
    pub const LICENSE_EXPIRED: &str = "LICENSE_EXPIRED";
    pub const INVALID_LICENSE_CODE: &str = "INVALID_LICENSE_CODE";

    // Task
    pub const TASK_NOT_FOUND: &str = "task_not_found";
    pub const TASK_KIND_IMMUTABLE: &str = "task_kind_immutable";
    pub const TASK_HAS_ACTIVE_RUN: &str = "task_has_active_run";

    // Run
    pub const RUN_ALREADY_ACTIVE: &str = "RUN_ALREADY_ACTIVE";
    pub const RUN_NOT_ACTIVE: &str = "RUN_NOT_ACTIVE";
    pub const ILLEGAL_TRANSITION: &str = "ILLEGAL_TRANSITION";
    pub const RUN_NOT_FOUND: &str = "run_not_found";

    // Task validation
    pub const GAUSSDB_SUB_MODE_REQUIRED: &str = "gaussdb_sub_mode_required";
    pub const UNKNOWN_GAUSSDB_SUB_MODE: &str = "unknown_gaussdb_sub_mode";
    pub const SYNC_MODE_INVALID_FOR_CATEGORY: &str = "sync_mode_invalid_for_category";
    pub const STRUCT_FILTER_REQUIRED: &str = "struct_filter_required";
    pub const INVALID_URL_SCHEME: &str = "invalid_url_scheme";
    pub const URL_SCHEME_ENGINE_MISMATCH: &str = "url_scheme_engine_mismatch";
    pub const PATH_OUTSIDE_SANDBOX: &str = "path_outside_sandbox";
    pub const ENDPOINT_HOST_BLOCKED: &str = "endpoint_host_blocked";
    pub const UNKNOWN_RESOURCE_GROUP: &str = "unknown_resource_group";

    // Resource Group
    pub const RESOURCE_GROUP_NAME_TAKEN: &str = "resource_group_name_taken";
    pub const DEFAULT_RESOURCE_GROUP_PROTECTED: &str = "default_resource_group_protected";
    pub const RESOURCE_GROUP_HAS_TASKS: &str = "resource_group_has_tasks";
    pub const RESOURCE_GROUP_NOT_FOUND: &str = "resource_group_not_found";

    // Export
    pub const UNSUPPORTED_EXPORT_FORMAT: &str = "unsupported_export_format";

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

            codes::NOT_FOUND
            | codes::TASK_NOT_FOUND
            | codes::RUN_NOT_FOUND
            | codes::RESOURCE_GROUP_NOT_FOUND => StatusCode::NOT_FOUND,

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
            | codes::TASK_VALIDATION_FAILED => StatusCode::UNPROCESSABLE_ENTITY,

            codes::CONFLICT
            | codes::LAST_ADMIN_PROTECTED
            | codes::USERNAME_TAKEN
            | codes::TASK_HAS_ACTIVE_RUN
            | codes::RUN_ALREADY_ACTIVE
            | codes::RUN_NOT_ACTIVE
            | codes::ILLEGAL_TRANSITION
            | codes::RESOURCE_GROUP_NAME_TAKEN
            | codes::DEFAULT_RESOURCE_GROUP_PROTECTED
            | codes::RESOURCE_GROUP_HAS_TASKS => StatusCode::CONFLICT,

            codes::LICENSE_EXPIRED
            | codes::INVALID_LICENSE_CODE
            | codes::UNSUPPORTED_EXPORT_FORMAT => StatusCode::BAD_REQUEST,

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
