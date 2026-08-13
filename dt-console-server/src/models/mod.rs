//! Domain model types for all 15 database entities.
//!
//! Each struct maps 1:1 to a database table row. JSON columns (like
//! `source_endpoint`, `filter_config`) are stored as `String` in the model
//! and (de)serialised via `serde_json` at the repository boundary.

use serde::{Deserialize, Serialize};

// ─── Auth DTOs ───────────────────────────────────────────────────────────

/// Request body for POST /api/auth/login.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Response body for POST /api/auth/login and GET /api/auth/me.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub username: String,
    pub display_name: String,
    pub role: String,
}

// ─── User DTOs ──────────────────────────────────────────────────────────

/// Request body for POST /api/users (create user).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: String,
    #[serde(default)]
    pub display_name: String,
}

/// Request body for PATCH /api/users/:id (update user).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

/// Response body for user endpoints — never includes password or password_hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserResponse {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub disabled: bool,
    pub created_at: String,
}

/// UserContext extracted from a validated session. Stored in request extensions
/// so handlers and middleware can access the authenticated user.
#[derive(Debug, Clone)]
pub struct UserContext {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub disabled: bool,
    /// Resource group the user is scoped to. None means all RGs (admin).
    pub resource_group_id: Option<String>,
}

// ─── User ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub display_name: String,
    pub role: String,
    pub disabled: bool,
    pub created_at: String,
    pub updated_at: String,
    /// Resource group the user is scoped to. None means all RGs.
    #[sqlx(default)]
    pub resource_group_id: Option<String>,
}

// ─── Session ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub token: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

// ─── ResourceGroup ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ResourceGroup {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

// ─── Task ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Task {
    pub id: String,
    pub task_id: String,
    pub name: String,
    pub kind: String,
    pub db_type_source: String,
    pub db_type_target: String,
    pub source_endpoint: String,
    pub target_endpoint: String,
    pub extractor_config: String,
    pub sinker_config: String,
    pub filter_config: String,
    pub router_config: String,
    pub parallelizer_config: String,
    pub pipeline_config: String,
    pub resumer_config: String,
    pub processor_config: String,
    pub runtime_config: String,
    pub metrics_config: String,
    pub resource_group_id: String,
    pub owner_user_id: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

// ─── Run ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Run {
    pub id: String,
    /// Nullable after task deletion (ON DELETE SET NULL on the FK).
    pub task_id: Option<String>,
    pub status: String,
    pub pid: Option<i64>,
    pub ini_path: Option<String>,
    pub log_dir: Option<String>,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub exit_code: Option<i64>,
    pub stop_method: Option<String>,
    /// Dynamically-allocated Prometheus metrics port in [9100, 9199].
    /// NULL for legacy rows or runs that failed before port allocation.
    #[sqlx(default)]
    pub metrics_port: Option<i64>,
    /// Set when this Run was started by resuming a `paused` Run: the id of
    /// that predecessor, whose `log_dir` supplied the resume position.
    #[sqlx(default)]
    pub resumed_from_run_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Response body for GET /api/runs/:run_id.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResponse {
    pub id: String,
    /// Nullable after task deletion (ON DELETE SET NULL on the FK).
    pub task_id: Option<String>,
    pub status: String,
    pub pid: Option<i64>,
    pub ini_path: Option<String>,
    pub log_dir: Option<String>,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub exit_code: Option<i64>,
    pub stop_method: Option<String>,
    pub position: Option<serde_json::Value>,
    /// Dynamically-allocated Prometheus metrics port in [9100, 9199].
    pub metrics_port: Option<i64>,
    /// Id of the `paused` Run this Run resumed from, if any.
    pub resumed_from_run_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Response body for POST /api/tasks/:id/start.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRunResponse {
    pub run_id: String,
}

/// Legal Run statuses in the state machine.
pub mod run_status {
    pub const PENDING: &str = "pending";
    pub const RUNNING: &str = "running";
    /// Pause requested: SIGTERM sent, engine still draining. Symmetric with
    /// `STOPPING` — the supervisor decides the terminal status when the
    /// process actually exits.
    pub const PAUSING: &str = "pausing";
    pub const PAUSED: &str = "paused";
    pub const STOPPING: &str = "stopping";
    pub const STOPPED: &str = "stopped";
    pub const FAILED: &str = "failed";

    /// All statuses that represent an "active" (non-terminal) Run.
    ///
    /// `paused` counts as active even though its engine process is gone: the
    /// Run still owns the position log a resume will start from, so the task
    /// must not be startable behind its back.
    pub fn is_active(status: &str) -> bool {
        matches!(status, PENDING | RUNNING | PAUSING | PAUSED | STOPPING)
    }

    /// All statuses that represent a terminal Run.
    pub fn is_terminal(status: &str) -> bool {
        matches!(status, STOPPED | FAILED)
    }
}

/// Check if a state transition is legal.
///
/// Legal transitions:
/// - pending → running
/// - running → pausing (pause requested: SIGTERM sent, engine draining)
/// - pausing → paused (engine exited 143 — the position log is trustworthy)
/// - pausing → failed (engine exited 4: the shutdown window expired, so the
///   position is not trustworthy and `paused` would be a lie)
/// - pausing → running (the pause was abandoned before its signal went out)
/// - pausing → stopping (stop overtakes a pause that will not converge —
///   without this, a wedged `pausing` Run would freeze its task forever)
/// - paused → running (a resume starts a *new* Run; the old one is closed
///   out separately as `stopped`/`resumed`)
/// - paused → stopped (discard a paused Run without ever resuming it)
/// - running → stopping
/// - paused → stopping
/// - stopping → stopped
/// - stopping → failed
/// - running → failed (unexpected exit)
/// - pending → failed (spawn failure)
pub fn is_legal_transition(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("pending", "running")
            | ("running", "pausing")
            | ("pausing", "paused")
            | ("pausing", "failed")
            | ("pausing", "stopped")
            | ("pausing", "running")
            | ("pausing", "stopping")
            | ("paused", "running")
            | ("paused", "stopped")
            | ("running", "stopping")
            | ("paused", "stopping")
            | ("stopping", "stopped")
            | ("stopping", "failed")
            | ("running", "failed")
            | ("pending", "failed")
    )
}

// ─── License ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct License {
    pub id: String,
    pub sku: String,
    pub max_tasks: i64,
    pub expire_at: Option<String>,
    pub activated_at: Option<String>,
    pub activation_code_hash: Option<String>,
    pub granted_to: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Request body for POST /api/license/activate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivateLicenseRequest {
    pub code: String,
}

/// Response body for GET /api/license.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseResponse {
    pub sku: String,
    pub max_tasks: i64,
    pub expire_at: Option<String>,
    pub status: String,
    pub granted_to: String,
    pub activated_at: Option<String>,
    pub current_tasks: i64,
}

/// Decoded activation code payload (internal use only).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationPayload {
    pub sku: String,
    pub max_tasks: i64,
    pub expire_at: String,
    pub granted_to: String,
    pub sig: String,
}

// ─── Task DTOs ────────────────────────────────────────────────────────────

/// Request body for POST /api/tasks (create task).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskRequest {
    #[serde(default)]
    pub name: String,
    pub kind: String,
    pub engine_source: String,
    #[serde(default)]
    pub engine_target: String,
    #[serde(default)]
    pub sub_mode: Option<String>,
    #[serde(default)]
    pub source_sub_mode: Option<String>,
    #[serde(default)]
    pub target_sub_mode: Option<String>,
    #[serde(default)]
    pub source_endpoint: serde_json::Value,
    #[serde(default)]
    pub target_endpoint: serde_json::Value,
    #[serde(default)]
    pub extractor: serde_json::Value,
    #[serde(default)]
    pub sinker: serde_json::Value,
    #[serde(default)]
    pub filter: serde_json::Value,
    #[serde(default)]
    pub router: serde_json::Value,
    #[serde(default)]
    pub parallelizer: serde_json::Value,
    #[serde(default)]
    pub pipeline: serde_json::Value,
    #[serde(default)]
    pub resumer: serde_json::Value,
    #[serde(default)]
    pub processor: serde_json::Value,
    #[serde(default)]
    pub runtime: serde_json::Value,
    #[serde(default)]
    pub metrics: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_group_id: Option<String>,
}

/// Request body for PATCH /api/tasks/:id (update task).
///
/// Kind is immutable — if present in the body it must match the persisted
/// value, or the handler returns 422.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_endpoint: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_endpoint: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extractor: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sinker: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub router: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallelizer: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumer: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processor: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_group_id: Option<String>,
}

/// Response body for task endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResponse {
    pub id: String,
    pub task_id: String,
    pub name: String,
    pub kind: String,
    pub db_type_source: String,
    pub db_type_target: String,
    pub source_endpoint: serde_json::Value,
    pub target_endpoint: serde_json::Value,
    pub extractor: serde_json::Value,
    pub sinker: serde_json::Value,
    pub filter: serde_json::Value,
    pub router: serde_json::Value,
    pub parallelizer: serde_json::Value,
    pub pipeline: serde_json::Value,
    pub resumer: serde_json::Value,
    pub processor: serde_json::Value,
    pub runtime: serde_json::Value,
    pub metrics: serde_json::Value,
    pub resource_group_id: String,
    pub owner_user_id: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

/// List response for GET /api/tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskListResponse {
    pub items: Vec<TaskResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

// ─── Resource Group DTOs ─────────────────────────────────────────────────

/// Request body for POST /api/resource_groups (create resource group).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateResourceGroupRequest {
    pub name: String,
}

/// Request body for PATCH /api/resource_groups/:id (update resource group).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResourceGroupRequest {
    pub name: Option<String>,
}

/// Response body for resource group endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGroupResponse {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

// ─── Alert ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Alert {
    pub id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub rule_id: Option<String>,
    pub metric_name: Option<String>,
    pub operator: Option<String>,
    pub threshold: Option<f64>,
    pub severity: String,
    pub value: Option<f64>,
    pub status: String,
    pub silenced: bool,
    pub fired_at: String,
    pub recovered_at: Option<String>,
    pub cleared_at: Option<String>,
    pub delivered_at: Option<String>,
    pub cleared_by: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
}

// ─── AlertRule ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub metric_name: String,
    pub operator: String,
    pub threshold: f64,
    pub recovery_threshold: Option<f64>,
    pub severity: String,
    pub dwell_secs: i64,
    pub channel_ids: String,
    pub enabled: bool,
    pub resource_group_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ─── AlarmChannel ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AlarmChannel {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub config: String,
    pub enabled: bool,
    pub resource_group_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ─── AlarmTemplate ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AlarmTemplate {
    pub id: String,
    pub name: String,
    pub subject_template: String,
    pub body_template: String,
    pub severity_mapping: String,
    pub created_at: String,
    pub updated_at: String,
}

// ─── OperateLog ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OperateLog {
    pub id: i64,
    pub actor: String,
    pub action: String,
    pub result: String,
    pub target: Option<String>,
    pub details: Option<String>,
    pub ip: Option<String>,
    pub created_at: String,
}

// ─── ControlLog ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ControlLog {
    pub id: i64,
    pub task_id: String,
    pub run_id: Option<String>,
    pub action: String,
    pub intent_or_result: String,
    pub operator_id: Option<String>,
    pub created_at: String,
}

/// Response DTO for GET /api/control_logs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlLogResponse {
    pub id: i64,
    pub task_id: String,
    pub run_id: Option<String>,
    pub action: String,
    pub phase: String,
    pub result: Option<String>,
    pub operator_id: Option<String>,
    pub ts: String,
}

impl ControlLog {
    /// Parse the `intent_or_result` field into (phase, result) components.
    ///
    /// - "intent" → ("intent", None)
    /// - "result:success" → ("result", Some("success"))
    /// - "result:error" → ("result", Some("error"))
    /// - "result:orphaned_by_restart" → ("result", Some("orphaned_by_restart"))
    pub fn phase_and_result(&self) -> (String, Option<String>) {
        if self.intent_or_result == "intent" {
            ("intent".to_string(), None)
        } else if let Some(rest) = self.intent_or_result.strip_prefix("result:") {
            ("result".to_string(), Some(rest.to_string()))
        } else {
            // Fallback for malformed data.
            (self.intent_or_result.clone(), None)
        }
    }

    /// Convert to the API response DTO.
    pub fn to_response(&self) -> ControlLogResponse {
        let (phase, result) = self.phase_and_result();
        ControlLogResponse {
            id: self.id,
            task_id: self.task_id.clone(),
            run_id: self.run_id.clone(),
            action: self.action.clone(),
            phase,
            result,
            operator_id: self.operator_id.clone(),
            ts: self.created_at.clone(),
        }
    }
}

/// List response for GET /api/control_logs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlLogListResponse {
    pub items: Vec<ControlLogResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

// ─── GlobalParam ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GlobalParam {
    pub id: String,
    pub key: String,
    pub value: String,
    pub created_at: String,
    pub updated_at: String,
}

// ─── MetricPoint ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MetricPoint {
    pub id: i64,
    pub task_id: String,
    pub run_id: String,
    pub metric_name: String,
    pub ts: String,
    pub value: f64,
}

// ─── DownsampledMetricPoint ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DownsampledMetricPoint {
    pub id: i64,
    pub task_id: String,
    pub run_id: String,
    pub metric_name: String,
    pub bucket_ts: String,
    pub bucket_secs: i64,
    pub value_mean: f64,
    pub value_min: f64,
    pub value_max: f64,
    pub sample_count: i64,
}

/// Response DTO for GET /api/runs/:id/metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricSeriesResponse {
    pub metric: String,
    pub data: Vec<MetricDataPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Vec<String>>,
}

/// A single point in a metric series response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricDataPoint {
    pub ts: String,
    pub value: f64,
}

// ─── SystemHost ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SystemHost {
    pub id: String,
    pub hostname: String,
    pub ip: String,
    pub status: String,
    pub last_heartbeat: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
