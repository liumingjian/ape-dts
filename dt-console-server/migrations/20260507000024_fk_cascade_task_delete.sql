-- Fix FK constraints that block Task deletion when runs/control_logs/metric_points
-- reference the task. Per VAL-TASK-015, control_logs are immutable audit rows
-- whose task_id must survive task deletion (denormalised FK). Per the feature
-- contract, runs.task_id becomes NULL on task deletion (ON DELETE SET NULL).
-- metric_points and downsampled_metric_points are operational data that should
-- not block task deletion either; we drop their FK to tasks(id).
--
-- SQLite does not support ALTER CONSTRAINT, so each table is recreated.

-- ── 1. runs: task_id nullable + ON DELETE SET NULL ─────────────────────

CREATE TABLE IF NOT EXISTS runs_new (
    id           TEXT PRIMARY KEY NOT NULL,
    task_id      TEXT,
    status       TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'paused', 'stopping', 'stopped', 'failed')),
    pid          INTEGER,
    ini_path     TEXT,
    log_dir      TEXT,
    started_at   TEXT,
    stopped_at   TEXT,
    exit_code    INTEGER,
    stop_method  TEXT,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE SET NULL
);

INSERT INTO runs_new SELECT * FROM runs;
DROP TABLE runs;
ALTER TABLE runs_new RENAME TO runs;

-- ── 2. control_logs: drop FK on task_id (audit data must survive deletion) ─

CREATE TABLE IF NOT EXISTS control_logs_new (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id         TEXT NOT NULL,
    run_id          TEXT,
    action          TEXT NOT NULL,
    intent_or_result TEXT NOT NULL CHECK (intent_or_result = 'intent' OR intent_or_result LIKE 'result:%'),
    operator_id     TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT INTO control_logs_new SELECT * FROM control_logs;
DROP TABLE control_logs;
ALTER TABLE control_logs_new RENAME TO control_logs;

-- ── 3. metric_points: drop FK on task_id ────────────────────────────────

CREATE TABLE IF NOT EXISTS metric_points_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id     TEXT NOT NULL,
    run_id      TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    ts          TEXT NOT NULL,
    value       REAL NOT NULL
);

INSERT INTO metric_points_new SELECT * FROM metric_points;
DROP TABLE metric_points;
ALTER TABLE metric_points_new RENAME TO metric_points;

CREATE INDEX IF NOT EXISTS idx_metric_points_task_run ON metric_points(task_id, run_id);
CREATE INDEX IF NOT EXISTS idx_metric_points_ts ON metric_points(ts);

-- ── 4. downsampled_metric_points: drop FK on task_id ────────────────────

CREATE TABLE IF NOT EXISTS downsampled_metric_points_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id     TEXT NOT NULL,
    run_id      TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    bucket_ts   TEXT NOT NULL,
    bucket_secs INTEGER NOT NULL,
    value_mean  REAL NOT NULL,
    value_min   REAL NOT NULL,
    value_max   REAL NOT NULL,
    sample_count INTEGER NOT NULL
);

INSERT INTO downsampled_metric_points_new SELECT * FROM downsampled_metric_points;
DROP TABLE downsampled_metric_points;
ALTER TABLE downsampled_metric_points_new RENAME TO downsampled_metric_points;

CREATE INDEX IF NOT EXISTS idx_downsampled_task_run_metric_ts
    ON downsampled_metric_points(task_id, run_id, metric_name, bucket_ts);
