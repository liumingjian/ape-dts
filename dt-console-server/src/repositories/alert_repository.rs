//! AlertRepository — CRUD operations for the `alerts` table.

use crate::models::Alert;
use sqlx::SqlitePool;

pub struct AlertRepository;

impl AlertRepository {
    /// Create a new alert.
    pub async fn create(pool: &SqlitePool, alert: &Alert) -> Result<Alert, sqlx::Error> {
        sqlx::query(
            "INSERT INTO alerts (id, task_id, run_id, rule_id, metric_name, operator,
             threshold, severity, value, status, fired_at, recovered_at, cleared_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&alert.id)
        .bind(&alert.task_id)
        .bind(&alert.run_id)
        .bind(&alert.rule_id)
        .bind(&alert.metric_name)
        .bind(&alert.operator)
        .bind(alert.threshold)
        .bind(&alert.severity)
        .bind(alert.value)
        .bind(&alert.status)
        .bind(&alert.fired_at)
        .bind(&alert.recovered_at)
        .bind(&alert.cleared_at)
        .bind(&alert.created_at)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &alert.id).await
    }

    /// Find an alert by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Alert, sqlx::Error> {
        sqlx::query_as("SELECT * FROM alerts WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// List alerts with optional filters.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<Alert>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM alerts ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }

    /// Update an alert (for status transitions).
    pub async fn update(pool: &SqlitePool, alert: &Alert) -> Result<Alert, sqlx::Error> {
        sqlx::query(
            "UPDATE alerts SET status = ?, recovered_at = ?, cleared_at = ?
             WHERE id = ?",
        )
        .bind(&alert.status)
        .bind(&alert.recovered_at)
        .bind(&alert.cleared_at)
        .bind(&alert.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &alert.id).await
    }
}
