//! RunRepository — CRUD operations for the `runs` table.

use crate::models::Run;
use sqlx::SqlitePool;

pub struct RunRepository;

impl RunRepository {
    /// Create a new run.
    pub async fn create(pool: &SqlitePool, run: &Run) -> Result<Run, sqlx::Error> {
        sqlx::query(
            "INSERT INTO runs (id, task_id, status, pid, ini_path, log_dir, started_at,
             stopped_at, exit_code, stop_method, metrics_port, resumed_from_run_id,
             created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&run.id)
        .bind(&run.task_id)
        .bind(&run.status)
        .bind(run.pid)
        .bind(&run.ini_path)
        .bind(&run.log_dir)
        .bind(&run.started_at)
        .bind(&run.stopped_at)
        .bind(run.exit_code)
        .bind(&run.stop_method)
        .bind(run.metrics_port)
        .bind(&run.resumed_from_run_id)
        .bind(&run.created_at)
        .bind(&run.updated_at)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &run.id).await
    }

    /// Find a run by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Run, sqlx::Error> {
        sqlx::query_as("SELECT * FROM runs WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// List runs for a given task.
    pub async fn list_by_task(pool: &SqlitePool, task_id: &str) -> Result<Vec<Run>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM runs WHERE task_id = ? ORDER BY created_at DESC")
            .bind(task_id)
            .fetch_all(pool)
            .await
    }

    /// Find the latest run for a given task regardless of status.
    ///
    /// Returns the most recent run (including terminal), or None if no run exists.
    pub async fn find_latest_by_task(
        pool: &SqlitePool,
        task_id: &str,
    ) -> Result<Option<Run>, sqlx::Error> {
        let runs =
            sqlx::query_as("SELECT * FROM runs WHERE task_id = ? ORDER BY created_at DESC LIMIT 1")
                .bind(task_id)
                .fetch_all(pool)
                .await?;

        Ok(runs.into_iter().next())
    }

    /// Find the active (non-terminal) run for a given task.
    ///
    /// Returns the most recent run whose status is in {pending, running, pausing, paused, stopping},
    /// or None if no active run exists.
    pub async fn find_active_by_task(
        pool: &SqlitePool,
        task_id: &str,
    ) -> Result<Option<Run>, sqlx::Error> {
        let runs = sqlx::query_as(
            "SELECT * FROM runs WHERE task_id = ? AND status IN ('pending', 'running', 'pausing', 'paused', 'stopping') ORDER BY created_at DESC LIMIT 1",
        )
        .bind(task_id)
        .fetch_all(pool)
        .await?;

        Ok(runs.into_iter().next())
    }

    /// Update a run.
    pub async fn update(pool: &SqlitePool, run: &Run) -> Result<Run, sqlx::Error> {
        sqlx::query(
            "UPDATE runs SET status = ?, pid = ?, ini_path = ?, log_dir = ?,
             started_at = ?, stopped_at = ?, exit_code = ?, stop_method = ?,
             metrics_port = ?, resumed_from_run_id = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&run.status)
        .bind(run.pid)
        .bind(&run.ini_path)
        .bind(&run.log_dir)
        .bind(&run.started_at)
        .bind(&run.stopped_at)
        .bind(run.exit_code)
        .bind(&run.stop_method)
        .bind(run.metrics_port)
        .bind(&run.resumed_from_run_id)
        .bind(&run.updated_at)
        .bind(&run.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &run.id).await
    }

    /// Move a run from `from` to `to`, but only if it is still in `from`.
    ///
    /// Returns whether the transition happened. A handler that read a Run,
    /// did some work and then wrote the whole row back can resurrect a Run
    /// the supervisor has already finalised — the supervisor's `stopped`,
    /// `exit_code` and `stopped_at` are silently overwritten by the stale
    /// snapshot, and nothing is left watching the process. Status changes
    /// that race the supervisor go through here instead.
    pub async fn transition_status(
        pool: &SqlitePool,
        run_id: &str,
        from: &str,
        to: &str,
    ) -> Result<bool, sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let result =
            sqlx::query("UPDATE runs SET status = ?, updated_at = ? WHERE id = ? AND status = ?")
                .bind(to)
                .bind(&now)
                .bind(run_id)
                .bind(from)
                .execute(pool)
                .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Check whether a task has an active (non-terminal) run.
    pub async fn has_active_run(pool: &SqlitePool, task_id: &str) -> Result<bool, sqlx::Error> {
        let active = Self::find_active_by_task(pool, task_id).await?;
        Ok(active.is_some())
    }

    /// List all runs across all tasks, most recent first.
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<Run>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM runs ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }

    /// List runs by a set of statuses (used for orchestrator restart reconciliation).
    pub async fn list_by_statuses(
        pool: &SqlitePool,
        statuses: &[&str],
    ) -> Result<Vec<Run>, sqlx::Error> {
        if statuses.is_empty() {
            return Ok(Vec::new());
        }
        // Build a parameterised IN clause: WHERE status IN (?, ?, ?)
        let placeholders: Vec<&str> = statuses.iter().map(|_| "?").collect();
        let sql = format!(
            "SELECT * FROM runs WHERE status IN ({}) ORDER BY created_at ASC",
            placeholders.join(",")
        );
        let mut query = sqlx::query_as::<_, Run>(&sql);
        for status in statuses {
            query = query.bind(*status);
        }
        query.fetch_all(pool).await
    }
}
