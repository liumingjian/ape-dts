//! ControlLogRepository — write and read `control_logs` rows.

use crate::models::ControlLog;
use sqlx::SqlitePool;

pub struct ControlLogRepository;

impl ControlLogRepository {
    /// Create a new control log entry.
    pub async fn create(pool: &SqlitePool, log: &ControlLog) -> Result<ControlLog, sqlx::Error> {
        sqlx::query(
            "INSERT INTO control_logs (task_id, run_id, action, intent_or_result, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&log.task_id)
        .bind(&log.run_id)
        .bind(&log.action)
        .bind(&log.intent_or_result)
        .bind(&log.created_at)
        .execute(pool)
        .await?;

        let row: ControlLog = sqlx::query_as("SELECT * FROM control_logs ORDER BY id DESC LIMIT 1")
            .fetch_one(pool)
            .await?;

        Ok(row)
    }

    /// List control logs, ordered by created_at DESC.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<ControlLog>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM control_logs ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }
}
