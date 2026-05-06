//! MetricPointRepository — write and read `metric_points` rows.

use crate::models::MetricPoint;
use sqlx::SqlitePool;

pub struct MetricPointRepository;

impl MetricPointRepository {
    /// Insert a metric point.
    pub async fn create(pool: &SqlitePool, mp: &MetricPoint) -> Result<MetricPoint, sqlx::Error> {
        sqlx::query(
            "INSERT INTO metric_points (task_id, run_id, metric_name, ts, value)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&mp.task_id)
        .bind(&mp.run_id)
        .bind(&mp.metric_name)
        .bind(&mp.ts)
        .bind(mp.value)
        .execute(pool)
        .await?;

        let row: MetricPoint =
            sqlx::query_as("SELECT * FROM metric_points ORDER BY id DESC LIMIT 1")
                .fetch_one(pool)
                .await?;

        Ok(row)
    }

    /// List metric points for a given run, optionally filtered by metric name and time range.
    pub async fn list_by_run(
        pool: &SqlitePool,
        run_id: &str,
    ) -> Result<Vec<MetricPoint>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM metric_points WHERE run_id = ? ORDER BY ts ASC")
            .bind(run_id)
            .fetch_all(pool)
            .await
    }
}
