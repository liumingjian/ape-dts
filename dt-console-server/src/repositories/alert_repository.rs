//! AlertRepository — CRUD operations for the `alerts` table.

use crate::models::Alert;
use sqlx::SqlitePool;

pub struct AlertRepository;

impl AlertRepository {
    /// Create a new alert.
    pub async fn create(pool: &SqlitePool, alert: &Alert) -> Result<Alert, sqlx::Error> {
        sqlx::query(
            "INSERT INTO alerts (id, task_id, run_id, rule_id, metric_name, operator,
             threshold, severity, value, status, silenced, fired_at, recovered_at,
             cleared_at, delivered_at, cleared_by, last_error, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&alert.id)
        .bind(&alert.task_id)
        .bind(&alert.run_id)
        .bind(&alert.rule_id)
        .bind(&alert.metric_name)
        .bind(&alert.operator)
        .bind(alert.threshold)
        .bind(&alert.severity)
        .bind(alert.value)
        .bind(&alert.status)
        .bind(alert.silenced)
        .bind(&alert.fired_at)
        .bind(&alert.recovered_at)
        .bind(&alert.cleared_at)
        .bind(&alert.delivered_at)
        .bind(&alert.cleared_by)
        .bind(&alert.last_error)
        .bind(&alert.created_at)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &alert.id).await
    }

    /// Find an alert by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Alert, sqlx::Error> {
        sqlx::query_as("SELECT * FROM alerts WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// List all alerts ordered by created_at desc.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<Alert>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM alerts ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }

    /// List alerts with optional filters.
    ///
    /// All filters are AND-combined. NULL / empty filters are ignored.
    /// Uses parameterized queries to prevent SQL injection (VAL-SEC-INJ-001).
    pub async fn list_filtered(
        pool: &SqlitePool,
        status: Option<&str>,
        level: Option<&str>,
        engine: Option<&str>,
        task_id: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<(Vec<Alert>, i64), sqlx::Error> {
        // Sanitise: only apply non-empty filter values.
        let status = status.filter(|s| !s.is_empty());
        let level = level.filter(|l| !l.is_empty());
        let task_id = task_id.filter(|t| !t.is_empty());
        let engine = engine.filter(|e| !e.is_empty());

        // Build WHERE clause with positional parameters.
        // Engine filter requires a subquery join with tasks.
        let mut conditions = Vec::new();
        let mut param_idx = 1u32; // 1-based for SQL parameter numbering

        if status.is_some() {
            conditions.push(format!("a.status = ?{param_idx}"));
            param_idx += 1;
        }
        if level.is_some() {
            conditions.push(format!("a.severity = ?{param_idx}"));
            param_idx += 1;
        }
        if task_id.is_some() {
            conditions.push(format!("a.task_id = ?{param_idx}"));
            param_idx += 1;
        }
        if engine.is_some() {
            conditions.push(format!(
                "EXISTS (SELECT 1 FROM tasks t WHERE t.id = a.task_id AND (t.db_type_source LIKE ?{param_idx} OR t.db_type_target LIKE ?{param_idx}))"
            ));
            param_idx += 1;
        }

        let where_sql = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Count query — same conditions, different alias.
        let mut count_conditions = Vec::new();
        let mut count_idx = 1u32;

        if status.is_some() {
            count_conditions.push(format!("status = ?{count_idx}"));
            count_idx += 1;
        }
        if level.is_some() {
            count_conditions.push(format!("severity = ?{count_idx}"));
            count_idx += 1;
        }
        if task_id.is_some() {
            count_conditions.push(format!("task_id = ?{count_idx}"));
            count_idx += 1;
        }
        if engine.is_some() {
            count_conditions.push(format!(
                "EXISTS (SELECT 1 FROM tasks t WHERE t.id = alerts.task_id AND (t.db_type_source LIKE ?{count_idx} OR t.db_type_target LIKE ?{count_idx}))"
            ));
            count_idx += 1;
        }

        let count_where_sql = if count_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", count_conditions.join(" AND "))
        };

        // Build and execute count query with bound params.
        let count_sql = format!("SELECT COUNT(*) FROM alerts {count_where_sql}");
        let mut count_query = sqlx::query_as::<_, (i64,)>(&count_sql);
        if let Some(s) = status {
            count_query = count_query.bind(s);
        }
        if let Some(l) = level {
            count_query = count_query.bind(l);
        }
        if let Some(t) = task_id {
            count_query = count_query.bind(t);
        }
        if let Some(e) = engine {
            let pattern = format!("%{e}%");
            count_query = count_query.bind(pattern);
        }
        let count: (i64,) = count_query.fetch_one(pool).await?;

        // Build and execute data query with bound params + LIMIT/OFFSET.
        let offset = (page - 1).max(0) * page_size;
        let data_sql = format!(
            "SELECT a.* FROM alerts a {where_sql} ORDER BY a.created_at DESC LIMIT ? OFFSET ?"
        );
        let mut data_query = sqlx::query_as::<_, Alert>(&data_sql);
        if let Some(s) = status {
            data_query = data_query.bind(s);
        }
        if let Some(l) = level {
            data_query = data_query.bind(l);
        }
        if let Some(t) = task_id {
            data_query = data_query.bind(t);
        }
        if let Some(e) = engine {
            let pattern = format!("%{e}%");
            data_query = data_query.bind(pattern);
        }
        data_query = data_query.bind(page_size).bind(offset);

        let items = data_query.fetch_all(pool).await?;

        Ok((items, count.0))
    }

    /// Update an alert (for status transitions and dispatch tracking).
    pub async fn update(pool: &SqlitePool, alert: &Alert) -> Result<Alert, sqlx::Error> {
        sqlx::query(
            "UPDATE alerts SET status = ?, silenced = ?, recovered_at = ?, cleared_at = ?,
             delivered_at = ?, cleared_by = ?, last_error = ?
             WHERE id = ?",
        )
        .bind(&alert.status)
        .bind(alert.silenced)
        .bind(&alert.recovered_at)
        .bind(&alert.cleared_at)
        .bind(&alert.delivered_at)
        .bind(&alert.cleared_by)
        .bind(&alert.last_error)
        .bind(&alert.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &alert.id).await
    }

    /// Find a firing alert by rule_id (for deduplication / dwell tracking).
    pub async fn find_firing_by_rule(
        pool: &SqlitePool,
        rule_id: &str,
        task_id: Option<&str>,
    ) -> Result<Option<Alert>, sqlx::Error> {
        let query = if task_id.is_some() {
            sqlx::query_as(
                "SELECT * FROM alerts WHERE rule_id = ? AND task_id = ? AND status = 'firing' ORDER BY fired_at DESC LIMIT 1",
            )
            .bind(rule_id)
            .bind(task_id)
        } else {
            sqlx::query_as(
                "SELECT * FROM alerts WHERE rule_id = ? AND status = 'firing' ORDER BY fired_at DESC LIMIT 1",
            )
            .bind(rule_id)
        };
        query.fetch_optional(pool).await
    }

    /// Find a cdc_stalled alert for a specific task (active firing).
    pub async fn find_cdc_stalled(
        pool: &SqlitePool,
        task_id: &str,
    ) -> Result<Option<Alert>, sqlx::Error> {
        sqlx::query_as(
            "SELECT * FROM alerts WHERE task_id = ? AND metric_name = 'cdc_stalled' AND status = 'firing' ORDER BY fired_at DESC LIMIT 1",
        )
        .bind(task_id)
        .fetch_optional(pool)
        .await
    }
}
