//! Domain model types for all 15 database entities.
//!
//! Each struct maps 1:1 to a database table row. JSON columns (like
//! `source_endpoint`, `filter_config`) are stored as `String` in the model
//! and (de)serialised via `serde_json` at the repository boundary.

use serde::{Deserialize, Serialize};

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
    pub task_id: String,
    pub status: String,
    pub pid: Option<i64>,
    pub ini_path: Option<String>,
    pub log_dir: Option<String>,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub exit_code: Option<i64>,
    pub stop_method: Option<String>,
    pub created_at: String,
    pub updated_at: String,
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
    pub fired_at: String,
    pub recovered_at: Option<String>,
    pub cleared_at: Option<String>,
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
    pub severity: String,
    pub dwell_secs: i64,
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
    pub created_at: String,
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
