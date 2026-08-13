-- Two changes to `runs`, both required by the pause/resume semantics in
-- ADR 0004:
--
-- 1. `pausing` joins the status CHECK — the symmetric counterpart of
--    `stopping`, written before the pause SIGTERM so the supervisor can tell
--    a requested pause from a requested stop when the exit code arrives.
-- 2. `resumed_from_run_id` links a resumed Run back to the `paused` Run whose
--    position log it continues. NULL for every Run started fresh and for all
--    legacy rows. Deliberately not a foreign key: a predecessor may be pruned
--    while its successor is still meaningful.
--
-- SQLite cannot alter a CHECK constraint, so the table is recreated — the
-- same shape the FK migration (0024) used, plus `metrics_port` (0025).

CREATE TABLE IF NOT EXISTS runs_new (
    id                  TEXT PRIMARY KEY NOT NULL,
    task_id             TEXT,
    status              TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'pausing', 'paused', 'stopping', 'stopped', 'failed')),
    pid                 INTEGER,
    ini_path            TEXT,
    log_dir             TEXT,
    started_at          TEXT,
    stopped_at          TEXT,
    exit_code           INTEGER,
    stop_method         TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    metrics_port        INTEGER NULL,
    resumed_from_run_id TEXT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE SET NULL
);

INSERT INTO runs_new (
    id, task_id, status, pid, ini_path, log_dir, started_at, stopped_at,
    exit_code, stop_method, created_at, updated_at, metrics_port
)
SELECT
    id, task_id, status, pid, ini_path, log_dir, started_at, stopped_at,
    exit_code, stop_method, created_at, updated_at, metrics_port
FROM runs;

DROP TABLE runs;
ALTER TABLE runs_new RENAME TO runs;
