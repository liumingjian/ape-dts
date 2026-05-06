//! TaskRepository — CRUD operations for the `tasks` table.

use crate::models::Task;
use sqlx::SqlitePool;

pub struct TaskRepository;

impl TaskRepository {
    /// Create a new task.
    pub async fn create(pool: &SqlitePool, task: &Task) -> Result<Task, sqlx::Error> {
        sqlx::query(
            "INSERT INTO tasks (id, task_id, name, kind, db_type_source, db_type_target,
             source_endpoint, target_endpoint, extractor_config, filter_config,
             router_config, parallelizer_config, pipeline_config, resumer_config,
             processor_config, runtime_config, metrics_config, resource_group_id,
             owner_user_id, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&task.id)
        .bind(&task.task_id)
        .bind(&task.name)
        .bind(&task.kind)
        .bind(&task.db_type_source)
        .bind(&task.db_type_target)
        .bind(&task.source_endpoint)
        .bind(&task.target_endpoint)
        .bind(&task.extractor_config)
        .bind(&task.filter_config)
        .bind(&task.router_config)
        .bind(&task.parallelizer_config)
        .bind(&task.pipeline_config)
        .bind(&task.resumer_config)
        .bind(&task.processor_config)
        .bind(&task.runtime_config)
        .bind(&task.metrics_config)
        .bind(&task.resource_group_id)
        .bind(&task.owner_user_id)
        .bind(&task.status)
        .bind(&task.created_at)
        .bind(&task.updated_at)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &task.id).await
    }

    /// Find a task by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Task, sqlx::Error> {
        sqlx::query_as("SELECT * FROM tasks WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// List all tasks.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<Task>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM tasks ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }

    /// Update a task (all mutable fields).
    pub async fn update(pool: &SqlitePool, task: &Task) -> Result<Task, sqlx::Error> {
        sqlx::query(
            "UPDATE tasks SET name = ?, source_endpoint = ?, target_endpoint = ?,
             extractor_config = ?, filter_config = ?, router_config = ?,
             parallelizer_config = ?, pipeline_config = ?, resumer_config = ?,
             processor_config = ?, runtime_config = ?, metrics_config = ?,
             resource_group_id = ?, status = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&task.name)
        .bind(&task.source_endpoint)
        .bind(&task.target_endpoint)
        .bind(&task.extractor_config)
        .bind(&task.filter_config)
        .bind(&task.router_config)
        .bind(&task.parallelizer_config)
        .bind(&task.pipeline_config)
        .bind(&task.resumer_config)
        .bind(&task.processor_config)
        .bind(&task.runtime_config)
        .bind(&task.metrics_config)
        .bind(&task.resource_group_id)
        .bind(&task.status)
        .bind(&task.updated_at)
        .bind(&task.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &task.id).await
    }

    /// Delete a task by id.
    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM tasks WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Count non-deleted tasks (for license cap enforcement).
    pub async fn count(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tasks")
            .fetch_one(pool)
            .await?;
        Ok(row.0)
    }
}
