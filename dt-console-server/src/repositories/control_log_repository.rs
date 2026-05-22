//! ControlLogRepository — write and read `control_logs` rows.

use crate::models::ControlLog;
use sqlx::SqlitePool;

pub struct ControlLogRepository;

/// Filter parameters for listing control logs.
pub struct ControlLogFilter<'a> {
    pub task_id: Option<&'a str>,
    pub action: Option<&'a str>,
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub page: i64,
    pub page_size: i64,
}

impl ControlLogRepository {
    /// Create a new control log entry.
    pub async fn create(pool: &SqlitePool, log: &ControlLog) -> Result<ControlLog, sqlx::Error> {
        sqlx::query(
            "INSERT INTO control_logs (task_id, run_id, action, intent_or_result, operator_id, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&log.task_id)
        .bind(&log.run_id)
        .bind(&log.action)
        .bind(&log.intent_or_result)
        .bind(&log.operator_id)
        .bind(&log.created_at)
        .execute(pool)
        .await?;

        let row: ControlLog = sqlx::query_as("SELECT * FROM control_logs ORDER BY id DESC LIMIT 1")
            .fetch_one(pool)
            .await?;

        Ok(row)
    }

    /// List control logs, ordered by created_at DESC.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<ControlLog>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM control_logs ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }

    /// List control logs with optional filters, ordered by created_at DESC.
    pub async fn list_filtered(
        pool: &SqlitePool,
        filter: &ControlLogFilter<'_>,
    ) -> Result<(Vec<ControlLog>, i64), sqlx::Error> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM control_logs \
             WHERE (?1 IS NULL OR task_id = ?1) \
             AND (?2 IS NULL OR action = ?2) \
             AND (?3 IS NULL OR created_at >= ?3) \
             AND (?4 IS NULL OR created_at <= ?4) \
             AND (?5 IS NULL OR run_id = ?5)",
        )
        .bind(filter.task_id)
        .bind(filter.action)
        .bind(filter.from)
        .bind(filter.to)
        .bind(filter.run_id)
        .fetch_one(pool)
        .await?;

        let offset = (filter.page - 1) * filter.page_size;
        let rows = sqlx::query_as(
            "SELECT * FROM control_logs \
             WHERE (?1 IS NULL OR task_id = ?1) \
             AND (?2 IS NULL OR action = ?2) \
             AND (?3 IS NULL OR created_at >= ?3) \
             AND (?4 IS NULL OR created_at <= ?4) \
             AND (?5 IS NULL OR run_id = ?5) \
             ORDER BY created_at DESC \
             LIMIT ?6 OFFSET ?7",
        )
        .bind(filter.task_id)
        .bind(filter.action)
        .bind(filter.from)
        .bind(filter.to)
        .bind(filter.run_id)
        .bind(filter.page_size)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok((rows, count.0))
    }

    /// Find orphaned intent rows: intents that have no matching result row.
    ///
    /// An intent is considered orphaned if there is no row with the same
    /// (task_id, run_id, action) where intent_or_result LIKE 'result:%'.
    pub async fn find_orphaned_intents(pool: &SqlitePool) -> Result<Vec<ControlLog>, sqlx::Error> {
        sqlx::query_as(
            "SELECT c.* FROM control_logs c \
             WHERE c.intent_or_result = 'intent' \
             AND NOT EXISTS ( \
                 SELECT 1 FROM control_logs r \
                 WHERE r.task_id = c.task_id \
                 AND (r.run_id = c.run_id OR (r.run_id IS NULL AND c.run_id IS NULL)) \
                 AND r.action = c.action \
                 AND r.intent_or_result LIKE 'result:%' \
             ) \
             ORDER BY c.created_at ASC",
        )
        .fetch_all(pool)
        .await
    }

    /// Finalise all orphaned intent rows by writing a synthetic result row.
    ///
    /// For each orphaned intent, writes a row with:
    /// - Same task_id, run_id, action
    /// - intent_or_result = "result:orphaned_by_restart"
    /// - Same operator_id as the orphaned intent
    /// - created_at = current UTC timestamp
    ///
    /// Returns the number of orphans finalised.
    pub async fn finalise_orphaned_intents(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
        let orphans = Self::find_orphaned_intents(pool).await?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut count = 0u64;

        for orphan in &orphans {
            let synthetic = ControlLog {
                id: 0,
                task_id: orphan.task_id.clone(),
                run_id: orphan.run_id.clone(),
                action: orphan.action.clone(),
                intent_or_result: "result:orphaned_by_restart".to_string(),
                operator_id: orphan.operator_id.clone(),
                created_at: now.clone(),
            };
            Self::create(pool, &synthetic).await?;
            count += 1;
        }

        Ok(count)
    }
}
