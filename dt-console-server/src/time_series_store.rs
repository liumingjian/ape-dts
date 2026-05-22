//! TimeSeriesStore — downsample after 24h, query API with cross-boundary stitching,
//! retention sweep on schedule.

use crate::models::{MetricDataPoint, MetricSeriesResponse};
use crate::repositories::metric_point_repository::MetricPointRepository;
use sqlx::SqlitePool;

/// Default downsample threshold in hours.
const DEFAULT_DOWNSAMPLE_THRESHOLD_HOURS: i64 = 24;

/// Default retention period for operate_logs / control_logs in days.
const DEFAULT_RETENTION_DAYS: i64 = 90;

/// Downsample raw metric points older than the threshold into bucketed rows.
///
/// For each (task_id, run_id, metric_name) group, aggregates raw rows
/// into 60-second buckets and writes them to `downsampled_metric_points`.
/// The original raw rows are deleted.
pub async fn downsample(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(DEFAULT_DOWNSAMPLE_THRESHOLD_HOURS);
    let cutoff_ts = cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    MetricPointRepository::downsample_old_raw(pool, &cutoff_ts).await
}

/// Run the retention sweep: downsample old metric_points, then delete
/// aged audit logs.
pub async fn retention_sweep(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let downsampled = downsample(pool).await?;
    let (op_del, ctrl_del) =
        MetricPointRepository::retention_sweep(pool, DEFAULT_RETENTION_DAYS).await?;
    let total_audit = op_del + ctrl_del;
    tracing::info!(
        event = "retention_sweep_completed",
        downsampled,
        audit_deleted = total_audit,
        "retention sweep completed"
    );
    Ok(())
}

/// Query time-series data for a given run and metric, with cross-boundary stitching.
///
/// If the query range spans both native (raw) and downsampled data,
/// this method stitches them together: downsampled buckets are used for
/// the older portion, raw points for the recent portion.
///
/// If `step` is provided, points are further aggregated into step-sized buckets.
pub async fn query_series(
    pool: &SqlitePool,
    run_id: &str,
    metric_name: &str,
    from: &str,
    to: &str,
    step: Option<i64>,
) -> Result<MetricSeriesResponse, sqlx::Error> {
    let downsample_cutoff =
        chrono::Utc::now() - chrono::Duration::hours(DEFAULT_DOWNSAMPLE_THRESHOLD_HOURS);
    let cutoff_ts = downsample_cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let mut all_points: Vec<MetricDataPoint> = Vec::new();
    let mut sources = Vec::new();

    // Query downsampled portion (from → min(to, cutoff)).
    let ds_to = if to < cutoff_ts.as_str() {
        to.to_string()
    } else {
        cutoff_ts.clone()
    };

    if from < ds_to.as_str() || from == ds_to.as_str() {
        let downsampled =
            MetricPointRepository::query_downsampled_range(pool, run_id, metric_name, from, &ds_to)
                .await?;
        if !downsampled.is_empty() {
            sources.push("downsampled".to_string());
            for dp in &downsampled {
                all_points.push(MetricDataPoint {
                    ts: dp.bucket_ts.clone(),
                    value: dp.value_mean,
                });
            }
        }
    }

    // Query native (raw) portion (max(from, cutoff) → to).
    let native_from = if from > cutoff_ts.as_str() {
        from.to_string()
    } else {
        cutoff_ts.clone()
    };

    if native_from.as_str() < to || native_from.as_str() == to {
        // Edge case: if from == to, query returns nothing.
        if native_from.as_str() != to {
            let native =
                MetricPointRepository::query_range(pool, run_id, metric_name, &native_from, to)
                    .await?;
            if !native.is_empty() {
                sources.push("native".to_string());
                for dp in &native {
                    all_points.push(MetricDataPoint {
                        ts: dp.ts.clone(),
                        value: dp.value,
                    });
                }
            }
        }
    }

    // Apply step-based aggregation if requested.
    if let Some(step_secs) = step {
        if step_secs > 0 {
            all_points = aggregate_by_step(&all_points, step_secs);
        }
    }

    // Sort by ts (should already be sorted, but ensure it after merging).
    all_points.sort_by(|a, b| a.ts.cmp(&b.ts));

    Ok(MetricSeriesResponse {
        metric: metric_name.to_string(),
        data: all_points,
        source: if sources.is_empty() {
            None
        } else {
            Some(sources)
        },
    })
}

/// Aggregate data points into step-sized buckets.
///
/// For each bucket, computes the mean of all points whose ts falls in the bucket.
fn aggregate_by_step(points: &[MetricDataPoint], step_secs: i64) -> Vec<MetricDataPoint> {
    if points.is_empty() || step_secs <= 0 {
        return points.to_vec();
    }

    let mut buckets: std::collections::BTreeMap<String, (f64, i64)> =
        std::collections::BTreeMap::new();

    for p in points {
        // Truncate ts to the start of the step bucket.
        let bucket_ts = truncate_ts_to_step(&p.ts, step_secs);
        let entry = buckets.entry(bucket_ts.clone()).or_insert((0.0, 0));
        entry.0 += p.value;
        entry.1 += 1;
    }

    buckets
        .into_iter()
        .map(|(ts, (sum, count))| MetricDataPoint {
            ts,
            value: sum / count as f64,
        })
        .collect()
}

/// Truncate an ISO-8601 timestamp to the start of a step bucket.
///
/// For step_secs=60, "2025-05-07T12:34:56.789Z" → "2025-05-07T12:34:00.000Z".
fn truncate_ts_to_step(ts: &str, step_secs: i64) -> String {
    // Parse the timestamp and truncate.
    let dt = match chrono::DateTime::parse_from_rfc3339(ts) {
        Ok(d) => d,
        Err(_) => return ts.to_string(), // If unparseable, return as-is.
    };

    let epoch_secs = dt.timestamp();
    let bucket_start = (epoch_secs / step_secs) * step_secs;
    let bucket_dt =
        chrono::DateTime::from_timestamp(bucket_start, 0).unwrap_or_else(chrono::Utc::now);
    bucket_dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Spawn the background retention sweep loop.
///
/// Runs once per day.
pub fn spawn_retention_loop(pool: sqlx::SqlitePool) {
    tokio::spawn(async move {
        loop {
            // Run once at startup, then every 24h.
            if let Err(e) = retention_sweep(&pool).await {
                tracing::warn!("retention sweep failed: {e}");
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(86400)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ts_to_step_rounds_down_to_60s() {
        let result = truncate_ts_to_step("2025-05-07T12:34:56.789Z", 60);
        assert_eq!(result, "2025-05-07T12:34:00.000Z");
    }

    #[test]
    fn truncate_ts_to_step_rounds_down_to_300s() {
        let result = truncate_ts_to_step("2025-05-07T12:07:56.789Z", 300);
        assert_eq!(result, "2025-05-07T12:05:00.000Z");
    }

    #[test]
    fn aggregate_by_step_groups_correctly() {
        let points = vec![
            MetricDataPoint {
                ts: "2025-05-07T12:00:10.000Z".into(),
                value: 10.0,
            },
            MetricDataPoint {
                ts: "2025-05-07T12:00:30.000Z".into(),
                value: 20.0,
            },
            MetricDataPoint {
                ts: "2025-05-07T12:01:05.000Z".into(),
                value: 30.0,
            },
        ];
        let result = aggregate_by_step(&points, 60);
        assert_eq!(result.len(), 2);
        assert!((result[0].value - 15.0).abs() < f64::EPSILON); // (10+20)/2
        assert!((result[1].value - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_by_step_empty_input() {
        let points: Vec<MetricDataPoint> = vec![];
        let result = aggregate_by_step(&points, 60);
        assert!(result.is_empty());
    }
}
