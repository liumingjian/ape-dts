//! MetricPointRepository — write and read `metric_points` rows,
//! plus downsample/query/retention operations for the TimeSeriesStore.

use crate::models::{DownsampledMetricPoint, MetricPoint};
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

    /// Insert multiple metric points in a single transaction.
    pub async fn create_batch(
        pool: &SqlitePool,
        points: &[MetricPoint],
    ) -> Result<(), sqlx::Error> {
        if points.is_empty() {
            return Ok(());
        }
        let mut tx = pool.begin().await?;
        for mp in points {
            sqlx::query(
                "INSERT INTO metric_points (task_id, run_id, metric_name, ts, value)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&mp.task_id)
            .bind(&mp.run_id)
            .bind(&mp.metric_name)
            .bind(&mp.ts)
            .bind(mp.value)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
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

    /// Query raw metric points for a given run, metric name, and time range.
    ///
    /// Returns points ordered by ts ASC.
    pub async fn query_range(
        pool: &SqlitePool,
        run_id: &str,
        metric_name: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<MetricPoint>, sqlx::Error> {
        sqlx::query_as(
            "SELECT * FROM metric_points
             WHERE run_id = ? AND metric_name = ? AND ts >= ? AND ts < ?
             ORDER BY ts ASC",
        )
        .bind(run_id)
        .bind(metric_name)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await
    }

    /// Check whether a metric name has any rows for a given run.
    pub async fn metric_name_exists_for_run(
        pool: &SqlitePool,
        run_id: &str,
        metric_name: &str,
    ) -> Result<bool, sqlx::Error> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM metric_points WHERE run_id = ? AND metric_name = ? LIMIT 1",
        )
        .bind(run_id)
        .bind(metric_name)
        .fetch_one(pool)
        .await?;
        Ok(row.0 > 0)
    }

    /// List distinct metric names for a given run.
    pub async fn list_metric_names_by_run(
        pool: &SqlitePool,
        run_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT metric_name FROM metric_points WHERE run_id = ? ORDER BY metric_name",
        )
        .bind(run_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    // ─── Downsample operations ─────────────────────────────────────────────

    /// Downsample raw metric points older than `cutoff_ts` into 60-second buckets.
    ///
    /// For each (task_id, run_id, metric_name, bucket) group, computes
    /// mean, min, max, and sample_count. Inserts into `downsampled_metric_points`
    /// and deletes the original raw rows.
    pub async fn downsample_old_raw(
        pool: &SqlitePool,
        cutoff_ts: &str,
    ) -> Result<u64, sqlx::Error> {
        // Aggregate raw points into buckets.
        let buckets: Vec<DownsampledMetricPoint> = sqlx::query_as(
            "SELECT
                0 AS id,
                task_id,
                run_id,
                metric_name,
                strftime('%Y-%m-%dT%H:%M:00.000Z', ts) AS bucket_ts,
                60 AS bucket_secs,
                AVG(value) AS value_mean,
                MIN(value) AS value_min,
                MAX(value) AS value_max,
                COUNT(*) AS sample_count
             FROM metric_points
             WHERE ts < ?
             GROUP BY task_id, run_id, metric_name, bucket_ts",
        )
        .bind(cutoff_ts)
        .fetch_all(pool)
        .await?;

        if buckets.is_empty() {
            return Ok(0);
        }

        let mut tx = pool.begin().await?;

        // Insert downsampled buckets.
        for b in &buckets {
            sqlx::query(
                "INSERT INTO downsampled_metric_points
                 (task_id, run_id, metric_name, bucket_ts, bucket_secs, value_mean, value_min, value_max, sample_count)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&b.task_id)
            .bind(&b.run_id)
            .bind(&b.metric_name)
            .bind(&b.bucket_ts)
            .bind(b.bucket_secs)
            .bind(b.value_mean)
            .bind(b.value_min)
            .bind(b.value_max)
            .bind(b.sample_count)
            .execute(&mut *tx)
            .await?;
        }

        // Delete the raw rows that were downsampled.
        let deleted = sqlx::query("DELETE FROM metric_points WHERE ts < ?")
            .bind(cutoff_ts)
            .execute(&mut *tx)
            .await?
            .rows_affected();

        tx.commit().await?;
        Ok(deleted)
    }

    /// Query downsampled metric points for a given run, metric name, and time range.
    pub async fn query_downsampled_range(
        pool: &SqlitePool,
        run_id: &str,
        metric_name: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<DownsampledMetricPoint>, sqlx::Error> {
        sqlx::query_as(
            "SELECT * FROM downsampled_metric_points
             WHERE run_id = ? AND metric_name = ? AND bucket_ts >= ? AND bucket_ts < ?
             ORDER BY bucket_ts ASC",
        )
        .bind(run_id)
        .bind(metric_name)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await
    }

    // ─── Retention sweep ──────────────────────────────────────────────────

    /// Delete operate_logs and control_logs older than `retention_days`.
    pub async fn retention_sweep(
        pool: &SqlitePool,
        retention_days: i64,
    ) -> Result<(u64, u64), sqlx::Error> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days);
        let cutoff_ts = cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let op_deleted = sqlx::query("DELETE FROM operate_logs WHERE created_at < ?")
            .bind(&cutoff_ts)
            .execute(pool)
            .await?
            .rows_affected();

        let ctrl_deleted = sqlx::query("DELETE FROM control_logs WHERE created_at < ?")
            .bind(&cutoff_ts)
            .execute(pool)
            .await?
            .rows_affected();

        Ok((op_deleted, ctrl_deleted))
    }
}
