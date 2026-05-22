//! OperateLogRepository — write and read `operate_logs` rows.

use crate::models::OperateLog;
use sqlx::SqlitePool;

pub struct OperateLogRepository;

impl OperateLogRepository {
    /// Create a new operate log entry.
    pub async fn create(pool: &SqlitePool, log: &OperateLog) -> Result<OperateLog, sqlx::Error> {
        sqlx::query(
            "INSERT INTO operate_logs (actor, action, result, target, details, ip, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&log.actor)
        .bind(&log.action)
        .bind(&log.result)
        .bind(&log.target)
        .bind(&log.details)
        .bind(&log.ip)
        .bind(&log.created_at)
        .execute(pool)
        .await?;

        let row: OperateLog = sqlx::query_as("SELECT * FROM operate_logs ORDER BY id DESC LIMIT 1")
            .fetch_one(pool)
            .await?;

        Ok(row)
    }

    /// List operate logs ordered by created_at DESC (no filtering).
    pub async fn list(pool: &SqlitePool) -> Result<Vec<OperateLog>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM operate_logs ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }

    /// List operate logs with optional filters and pagination.
    ///
    /// Filters are AND-combined. `from` and `to` are ISO-8601 timestamp
    /// bounds (inclusive). Results are ordered by `created_at DESC`.
    ///
    /// Returns `(items, total_count)` where `total_count` is the number of
    /// rows matching the filters (ignoring pagination).
    #[allow(clippy::too_many_arguments)]
    pub async fn list_filtered(
        pool: &SqlitePool,
        actor: Option<&str>,
        action: Option<&str>,
        result: Option<&str>,
        from: Option<&str>,
        to: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<(Vec<OperateLog>, i64), sqlx::Error> {
        // Build dynamic WHERE clause
        let mut conditions: Vec<String> = Vec::new();
        let mut param_count: u32 = 0;

        let actor_param = if let Some(v) = actor {
            param_count += 1;
            conditions.push(format!("actor = ?{param_count}"));
            Some(v.to_string())
        } else {
            None
        };

        let action_param = if let Some(v) = action {
            param_count += 1;
            conditions.push(format!("action = ?{param_count}"));
            Some(v.to_string())
        } else {
            None
        };

        let result_param = if let Some(v) = result {
            param_count += 1;
            conditions.push(format!("result = ?{param_count}"));
            Some(v.to_string())
        } else {
            None
        };

        let from_param = if let Some(v) = from {
            param_count += 1;
            conditions.push(format!("created_at >= ?{param_count}"));
            Some(v.to_string())
        } else {
            None
        };

        let to_param = if let Some(v) = to {
            param_count += 1;
            conditions.push(format!("created_at <= ?{param_count}"));
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
        let count_sql = format!("SELECT COUNT(*) FROM operate_logs {where_clause}");
        let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
        if let Some(ref v) = actor_param {
            count_query = count_query.bind(v);
        }
        if let Some(ref v) = action_param {
            count_query = count_query.bind(v);
        }
        if let Some(ref v) = result_param {
            count_query = count_query.bind(v);
        }
        if let Some(ref v) = from_param {
            count_query = count_query.bind(v);
        }
        if let Some(ref v) = to_param {
            count_query = count_query.bind(v);
        }

        let total = count_query.fetch_one(pool).await?;

        // Data query with pagination
        let offset = (page - 1) * page_size;
        let data_sql = format!(
            "SELECT * FROM operate_logs {where_clause} ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
            param_count + 1,
            param_count + 2
        );

        let mut data_query = sqlx::query_as::<_, OperateLog>(&data_sql);
        if let Some(ref v) = actor_param {
            data_query = data_query.bind(v);
        }
        if let Some(ref v) = action_param {
            data_query = data_query.bind(v);
        }
        if let Some(ref v) = result_param {
            data_query = data_query.bind(v);
        }
        if let Some(ref v) = from_param {
            data_query = data_query.bind(v);
        }
        if let Some(ref v) = to_param {
            data_query = data_query.bind(v);
        }
        data_query = data_query.bind(page_size);
        data_query = data_query.bind(offset);

        let items = data_query.fetch_all(pool).await?;

        Ok((items, total))
    }
}
