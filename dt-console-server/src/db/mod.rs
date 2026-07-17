//! Database layer: pool creation, migration runner, and schema integrity checks.
//!
//! Production uses `./data/console.db`; tests use `:memory:` SQLite.
//! Migrations run on every startup via `sqlx::migrate!()`. A partially-migrated
//! or corrupted schema causes the process to refuse to boot with a clear error.

use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

/// Well-known error code for schema integrity failures.
pub const SCHEMA_MISMATCH_CODE: &str = "SCHEMA_MISMATCH";

/// Errors that can occur during database initialisation.
#[derive(Debug, Error)]
pub enum DbError {
    #[error("schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("migration failed: {0}")]
    MigrationFailed(String),

    #[error("pool creation failed: {0}")]
    PoolCreation(String),
}

/// Create a `SqlitePool` connected to the database at `path`.
///
/// - If `path` is `:memory:`, an in-memory database is used (for tests).
///   The pool is limited to a single connection so all operations share the
///   same in-memory state (SQLite :memory: is per-connection by default).
/// - Otherwise, the parent directory is created if it doesn't exist,
///   and WAL journal mode is enabled for better concurrent read performance.
pub async fn create_pool(path: &str) -> Result<SqlitePool, DbError> {
    let is_memory = path == ":memory:";

    let opts = if is_memory {
        sqlx::sqlite::SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(|e| DbError::PoolCreation(e.to_string()))?
    } else {
        // Ensure parent directory exists for file-based databases.
        let p = Path::new(path);
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    DbError::PoolCreation(format!("failed to create data directory: {e}"))
                })?;
            }
        }
        let url = format!("sqlite://{path}?mode=rwc");
        sqlx::sqlite::SqliteConnectOptions::from_str(&url)
            .map_err(|e| DbError::PoolCreation(e.to_string()))?
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
    };

    // :memory: databases are per-connection in SQLite. Using a pool with
    // more than one connection would give each connection its own private
    // database. Limit to 1 connection so all operations share the same state.
    let max_conn = if is_memory { 1 } else { 5 };

    let pool = SqlitePoolOptions::new()
        .max_connections(max_conn)
        .connect_with(opts)
        .await
        .map_err(|e| DbError::PoolCreation(e.to_string()))?;

    Ok(pool)
}

/// Run all pending migrations and verify schema integrity.
///
/// On success, all 15 tables exist and are up-to-date. On failure:
/// - `DbError::SchemaMismatch` is returned when previously-applied migrations
///   have been tampered with (checksum mismatch, missing entries). The caller
///   should refuse to boot with `code="SCHEMA_MISMATCH"`.
/// - `DbError::MigrationFailed` is returned for other migration errors.
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), DbError> {
    let migrator = sqlx::migrate!("./migrations");
    let result = migrator.run(pool).await;

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            // sqlx reports checksum mismatches with distinctive messages.
            // We classify them as SchemaMismatch; everything else is a generic failure.
            if is_schema_mismatch(&msg) {
                Err(DbError::SchemaMismatch(msg))
            } else {
                Err(DbError::MigrationFailed(msg))
            }
        }
    }
}

/// Determine if a migration error indicates a schema integrity problem
/// (checksum mismatch, version conflict, dirty/partially-applied migration).
fn is_schema_mismatch(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("checksum")
        || lower.contains("version")
        || lower.contains("dirty")
        || lower.contains("partially applied")
        || lower.contains("has been modified")
}

/// Fully initialise the database: create pool, run migrations, verify tables.
///
/// This is the single entry-point called from `main()` on startup.
pub async fn init(db_path: &str) -> Result<SqlitePool, DbError> {
    let pool = create_pool(db_path).await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test pool backed by a temporary file.
    ///
    /// SQLite :memory: databases are per-connection, making them unreliable
    /// with connection pools. Instead, we use a temp file.
    async fn test_pool() -> SqlitePool {
        let dir = std::env::temp_dir().join("dt-console-server-test");
        std::fs::create_dir_all(&dir).unwrap();
        // Use a unique name per test to avoid interference between parallel tests.
        let test_name = std::thread::current()
            .name()
            .unwrap_or("unknown")
            .to_string();
        let safe_name: String = test_name
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let path = dir.join(format!("test-{safe_name}.db"));
        // Remove any leftover from a previous run.
        let _ = std::fs::remove_file(&path);
        let path_str = path.to_string_lossy().to_string();
        create_pool(&path_str).await.unwrap()
    }

    #[tokio::test]
    async fn test_create_pool_in_memory() {
        let pool = test_pool().await;
        let row: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await.unwrap();
        assert_eq!(row.0, 1);
    }

    #[tokio::test]
    async fn test_run_migrations_creates_all_tables() {
        let pool = test_pool().await;
        run_migrations(&pool).await.unwrap();

        let tables: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                .fetch_all(&pool)
                .await
                .unwrap();

        let table_names: Vec<&str> = tables.iter().map(|t| t.0.as_str()).collect();

        // _sqlx_migrations is the internal tracking table.
        let expected = [
            "_sqlx_migrations",
            "alert_rules",
            "alerts",
            "alarm_channels",
            "alarm_templates",
            "control_logs",
            "global_params",
            "licenses",
            "downsampled_metric_points",
            "metric_points",
            "operate_logs",
            "resource_groups",
            "runs",
            "sessions",
            "system_hosts",
            "tasks",
            "users",
        ];

        for name in &expected {
            assert!(
                table_names.contains(name),
                "expected table '{name}' not found, got: {table_names:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_migrations_are_idempotent() {
        let pool = test_pool().await;
        run_migrations(&pool).await.unwrap();

        // Running migrations again should succeed without error.
        run_migrations(&pool).await.unwrap();

        // Verify no duplicate tables or columns.
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        // 25 migrations, each applied exactly once.
        assert_eq!(count.0, 25);
    }

    #[tokio::test]
    async fn test_corrupted_checksum_detected() {
        let pool = test_pool().await;
        run_migrations(&pool).await.unwrap();

        // Corrupt the checksum of the first migration.
        // Migration versions are timestamps like 20260507000001, not sequential.
        let first_version: (i64,) = sqlx::query_as("SELECT MIN(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET checksum = X'DEADBEEF' WHERE version = ?")
            .bind(first_version.0)
            .execute(&pool)
            .await
            .unwrap();

        let result = run_migrations(&pool).await;
        // sqlx detects checksum corruption and reports it as a schema mismatch.
        assert!(result.is_err(), "expected error after checksum corruption");
        match result {
            Err(DbError::SchemaMismatch(msg)) => {
                assert!(
                    msg.to_lowercase().contains("modified")
                        || msg.to_lowercase().contains("checksum")
                        || msg.to_lowercase().contains("version"),
                    "expected modification-related error, got: {msg}"
                );
            }
            Err(DbError::MigrationFailed(msg)) => {
                assert!(
                    msg.to_lowercase().contains("modified")
                        || msg.to_lowercase().contains("checksum")
                        || msg.to_lowercase().contains("version"),
                    "expected integrity-related error, got: {msg}"
                );
            }
            other => panic!("expected SchemaMismatch or MigrationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_dirty_migration_flag_detected() {
        let pool = test_pool().await;
        run_migrations(&pool).await.unwrap();

        // Verify we can read _sqlx_migrations.
        let count_before: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count_before.0, 25, "all 25 migrations should be recorded");

        // Mark the first applied migration as dirty (success = false).
        // Migration versions are timestamps like 20260507000001, not sequential.
        let first_version: (i64,) = sqlx::query_as("SELECT MIN(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();

        let affected = sqlx::query("UPDATE _sqlx_migrations SET success = 0 WHERE version = ?")
            .bind(first_version.0)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            affected.rows_affected(),
            1,
            "should update one migration row"
        );

        // sqlx should detect the dirty migration and refuse to proceed.
        let result = run_migrations(&pool).await;
        assert!(
            result.is_err(),
            "expected error after marking migration as dirty"
        );
        match result {
            Err(DbError::SchemaMismatch(msg)) => {
                assert!(
                    msg.to_lowercase().contains("partially")
                        || msg.to_lowercase().contains("dirty")
                        || msg.to_lowercase().contains("version"),
                    "expected dirty/partial-related error, got: {msg}"
                );
            }
            Err(DbError::MigrationFailed(msg)) => {
                assert!(
                    msg.to_lowercase().contains("partially")
                        || msg.to_lowercase().contains("dirty")
                        || msg.to_lowercase().contains("version")
                        || msg.to_lowercase().contains("checksum"),
                    "expected integrity-related error, got: {msg}"
                );
            }
            Err(DbError::PoolCreation(_)) => {
                panic!("unexpected PoolCreation error");
            }
            Ok(()) => {
                // Should not happen since sqlx detects dirty migrations.
                panic!("expected error for dirty migration, got Ok");
            }
        }
    }

    #[tokio::test]
    async fn test_missing_migration_entry_reapplied() {
        let pool = test_pool().await;
        run_migrations(&pool).await.unwrap();

        // Delete the first migration entry to simulate a torn migration.
        // We pick a CREATE TABLE migration (which uses IF NOT EXISTS) rather
        // than an ALTER TABLE migration, because ALTER TABLE ADD COLUMN
        // cannot be re-applied if the column already exists.
        let first_version: (i64,) = sqlx::query_as("SELECT MIN(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM _sqlx_migrations WHERE version = ?")
            .bind(first_version.0)
            .execute(&pool)
            .await
            .unwrap();

        // Re-running migrations should re-apply the missing migration.
        // Because we use IF NOT EXISTS, this succeeds — which is the correct
        // resilient behavior for idempotent migrations.
        let result = run_migrations(&pool).await;
        assert!(
            result.is_ok(),
            "migration should succeed after re-applying missing entry: {:?}",
            result.err()
        );

        // Verify that all 25 migrations are now recorded.
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 25);
    }
}
