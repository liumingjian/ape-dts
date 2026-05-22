//! AlarmTemplateRepository — CRUD operations for the `alarm_templates` table.

use crate::models::AlarmTemplate;
use sqlx::SqlitePool;

pub struct AlarmTemplateRepository;

impl AlarmTemplateRepository {
    /// Create a new alarm template.
    pub async fn create(
        pool: &SqlitePool,
        t: &AlarmTemplate,
    ) -> Result<AlarmTemplate, sqlx::Error> {
        sqlx::query(
            "INSERT INTO alarm_templates (id, name, subject_template, body_template, severity_mapping, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&t.id)
        .bind(&t.name)
        .bind(&t.subject_template)
        .bind(&t.body_template)
        .bind(&t.severity_mapping)
        .bind(&t.created_at)
        .bind(&t.updated_at)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &t.id).await
    }

    /// Find an alarm template by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<AlarmTemplate, sqlx::Error> {
        sqlx::query_as("SELECT * FROM alarm_templates WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// List all alarm templates.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<AlarmTemplate>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM alarm_templates ORDER BY created_at ASC")
            .fetch_all(pool)
            .await
    }

    /// Update an alarm template.
    pub async fn update(
        pool: &SqlitePool,
        t: &AlarmTemplate,
    ) -> Result<AlarmTemplate, sqlx::Error> {
        sqlx::query(
            "UPDATE alarm_templates SET name = ?, subject_template = ?, body_template = ?,
             severity_mapping = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&t.name)
        .bind(&t.subject_template)
        .bind(&t.body_template)
        .bind(&t.severity_mapping)
        .bind(&t.updated_at)
        .bind(&t.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &t.id).await
    }

    /// Delete an alarm template by id.
    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM alarm_templates WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}
