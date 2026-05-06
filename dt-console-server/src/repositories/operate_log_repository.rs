//! OperateLogRepository — write and read `operate_logs` rows.

use crate::models::OperateLog;
use sqlx::SqlitePool;

pub struct OperateLogRepository;

impl OperateLogRepository {
    /// Create a new operate log entry.
    pub async fn create(pool: &SqlitePool, log: &OperateLog) -> Result<OperateLog, sqlx::Error> {
        sqlx::query(
            "INSERT INTO operate_logs (actor, action, result, target, details, ip, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&log.actor)
        .bind(&log.action)
        .bind(&log.result)
        .bind(&log.target)
        .bind(&log.details)
        .bind(&log.ip)
        .bind(&log.created_at)
        .execute(pool)
        .await?;

        let row: OperateLog = sqlx::query_as("SELECT * FROM operate_logs ORDER BY id DESC LIMIT 1")
            .fetch_one(pool)
            .await?;

        Ok(row)
    }

    /// List operate logs with optional filters, ordered by created_at DESC.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<OperateLog>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM operate_logs ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }
}
