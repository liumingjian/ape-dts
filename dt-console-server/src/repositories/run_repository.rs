//! RunRepository — CRUD operations for the `runs` table.

use crate::models::Run;
use sqlx::SqlitePool;

pub struct RunRepository;

impl RunRepository {
    /// Create a new run.
    pub async fn create(pool: &SqlitePool, run: &Run) -> Result<Run, sqlx::Error> {
        sqlx::query(
            "INSERT INTO runs (id, task_id, status, pid, ini_path, log_dir, started_at,
             stopped_at, exit_code, stop_method, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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

    /// Update a run.
    pub async fn update(pool: &SqlitePool, run: &Run) -> Result<Run, sqlx::Error> {
        sqlx::query(
            "UPDATE runs SET status = ?, pid = ?, ini_path = ?, log_dir = ?,
             started_at = ?, stopped_at = ?, exit_code = ?, stop_method = ?,
             updated_at = ?
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
        .bind(&run.updated_at)
        .bind(&run.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &run.id).await
    }
}
