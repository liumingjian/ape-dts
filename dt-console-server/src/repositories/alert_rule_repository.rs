//! AlertRuleRepository — CRUD operations for the `alert_rules` table.

use crate::models::AlertRule;
use sqlx::SqlitePool;

pub struct AlertRuleRepository;

impl AlertRuleRepository {
    /// Create a new alert rule.
    pub async fn create(pool: &SqlitePool, rule: &AlertRule) -> Result<AlertRule, sqlx::Error> {
        sqlx::query(
            "INSERT INTO alert_rules (id, name, metric_name, operator, threshold, severity,
             dwell_secs, enabled, resource_group_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&rule.id)
        .bind(&rule.name)
        .bind(&rule.metric_name)
        .bind(&rule.operator)
        .bind(rule.threshold)
        .bind(&rule.severity)
        .bind(rule.dwell_secs)
        .bind(rule.enabled)
        .bind(&rule.resource_group_id)
        .bind(&rule.created_at)
        .bind(&rule.updated_at)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &rule.id).await
    }

    /// Find an alert rule by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<AlertRule, sqlx::Error> {
        sqlx::query_as("SELECT * FROM alert_rules WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// List all alert rules.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<AlertRule>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM alert_rules ORDER BY created_at ASC")
            .fetch_all(pool)
            .await
    }

    /// Update an alert rule.
    pub async fn update(pool: &SqlitePool, rule: &AlertRule) -> Result<AlertRule, sqlx::Error> {
        sqlx::query(
            "UPDATE alert_rules SET name = ?, metric_name = ?, operator = ?, threshold = ?,
             severity = ?, dwell_secs = ?, enabled = ?, resource_group_id = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&rule.name)
        .bind(&rule.metric_name)
        .bind(&rule.operator)
        .bind(rule.threshold)
        .bind(&rule.severity)
        .bind(rule.dwell_secs)
        .bind(rule.enabled)
        .bind(&rule.resource_group_id)
        .bind(&rule.updated_at)
        .bind(&rule.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &rule.id).await
    }

    /// Delete an alert rule by id.
    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM alert_rules WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}
