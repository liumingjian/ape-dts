//! TaskRepository — CRUD operations + filtered list for the `tasks` table.

use crate::models::Task;
use sqlx::SqlitePool;

pub struct TaskRepository;

impl TaskRepository {
    /// Create a new task.
    pub async fn create(pool: &SqlitePool, task: &Task) -> Result<Task, sqlx::Error> {
        sqlx::query(
            "INSERT INTO tasks (id, task_id, name, kind, db_type_source, db_type_target,
             source_endpoint, target_endpoint, extractor_config, sinker_config, filter_config,
             router_config, parallelizer_config, pipeline_config, resumer_config,
             processor_config, runtime_config, metrics_config, resource_group_id,
             owner_user_id, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .bind(&task.sinker_config)
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

    /// List all tasks (unfiltered, ordered by created_at DESC).
    pub async fn list(pool: &SqlitePool) -> Result<Vec<Task>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM tasks ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }

    /// List tasks with AND-combined filters and pagination.
    ///
    /// Filters:
    /// - `category` — matches `kind` column
    /// - `status` — matches `status` column
    /// - `engine` — matches `db_type_source` OR `db_type_target`
    /// - `q` — full-text on `task_id` or `name` (LIKE %q%)
    /// - `resource_group` — matches `resource_group_id`
    ///
    /// Returns `(items, total_count)`.
    #[allow(clippy::too_many_arguments)]
    pub async fn list_filtered(
        pool: &SqlitePool,
        category: Option<&str>,
        status: Option<&str>,
        engine: Option<&str>,
        q: Option<&str>,
        resource_group: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<(Vec<Task>, i64), sqlx::Error> {
        let mut conditions: Vec<String> = Vec::new();
        let mut param_idx: u32 = 0;

        // Category filter
        let category_param = if let Some(v) = category {
            param_idx += 1;
            conditions.push(format!("kind = ?{param_idx}"));
            Some(v.to_string())
        } else {
            None
        };

        // Status filter
        let status_param = if let Some(v) = status {
            param_idx += 1;
            conditions.push(format!("status = ?{param_idx}"));
            Some(v.to_string())
        } else {
            None
        };

        // Engine filter (source OR target)
        let engine_param = if let Some(v) = engine {
            param_idx += 1;
            conditions.push(format!(
                "(db_type_source = ?{param_idx} OR db_type_target = ?{param_idx})"
            ));
            Some(v.to_string())
        } else {
            None
        };

        // Full-text query
        let q_param = if let Some(v) = q {
            if !v.is_empty() {
                param_idx += 1;
                conditions.push(format!(
                    "(task_id LIKE ?{param_idx} OR name LIKE ?{param_idx})"
                ));
                Some(format!("%{v}%"))
            } else {
                None
            }
        } else {
            None
        };

        // Resource group filter
        let rg_param = if let Some(v) = resource_group {
            param_idx += 1;
            conditions.push(format!("resource_group_id = ?{param_idx}"));
            Some(v.to_string())
        } else {
            None
        };

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Count query
        let count_sql = format!("SELECT COUNT(*) FROM tasks {where_clause}");
        let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
        if let Some(ref v) = category_param {
            count_query = count_query.bind(v);
        }
        if let Some(ref v) = status_param {
            count_query = count_query.bind(v);
        }
        if let Some(ref v) = engine_param {
            count_query = count_query.bind(v);
        }
        if let Some(ref v) = q_param {
            count_query = count_query.bind(v);
        }
        if let Some(ref v) = rg_param {
            count_query = count_query.bind(v);
        }

        let total = count_query.fetch_one(pool).await?;

        // Data query with pagination
        let offset = (page - 1) * page_size;
        let data_sql = format!(
            "SELECT * FROM tasks {where_clause} ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
            param_idx + 1,
            param_idx + 2
        );

        let mut data_query = sqlx::query_as::<_, Task>(&data_sql);
        if let Some(ref v) = category_param {
            data_query = data_query.bind(v);
        }
        if let Some(ref v) = status_param {
            data_query = data_query.bind(v);
        }
        if let Some(ref v) = engine_param {
            data_query = data_query.bind(v);
        }
        if let Some(ref v) = q_param {
            data_query = data_query.bind(v);
        }
        if let Some(ref v) = rg_param {
            data_query = data_query.bind(v);
        }
        data_query = data_query.bind(page_size);
        data_query = data_query.bind(offset);

        let items = data_query.fetch_all(pool).await?;

        Ok((items, total))
    }

    /// Update a task (all mutable fields).
    pub async fn update(pool: &SqlitePool, task: &Task) -> Result<Task, sqlx::Error> {
        sqlx::query(
            "UPDATE tasks SET name = ?, source_endpoint = ?, target_endpoint = ?,
             extractor_config = ?, sinker_config = ?, filter_config = ?, router_config = ?,
             parallelizer_config = ?, pipeline_config = ?, resumer_config = ?,
             processor_config = ?, runtime_config = ?, metrics_config = ?,
             resource_group_id = ?, status = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&task.name)
        .bind(&task.source_endpoint)
        .bind(&task.target_endpoint)
        .bind(&task.extractor_config)
        .bind(&task.sinker_config)
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

    /// Count tasks belonging to a specific resource group.
    pub async fn count_by_resource_group(
        pool: &SqlitePool,
        rg_id: &str,
    ) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tasks WHERE resource_group_id = ?")
            .bind(rg_id)
            .fetch_one(pool)
            .await?;
        Ok(row.0)
    }
}
