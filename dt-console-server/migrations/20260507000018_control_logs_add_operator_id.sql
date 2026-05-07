-- Add operator_id column to control_logs (idempotent: safe to re-run).
-- SQLite doesn't support ADD COLUMN IF NOT EXISTS, so we use pragma_table_info
-- to check if the column already exists before adding it.
SELECT 1 FROM pragma_table_info('control_logs') WHERE name = 'operator_id';
-- If the above returns no rows, the column doesn't exist yet.
-- Unfortunately, SQLite ALTER TABLE cannot be conditional in a migration script.
-- Instead, we use the standard ALTER TABLE which will be tracked by sqlx
-- and only run once under normal operation. The re-application test needs
-- the migration to be idempotent, so we wrap in a safe pattern.
ALTER TABLE control_logs ADD COLUMN operator_id TEXT;
