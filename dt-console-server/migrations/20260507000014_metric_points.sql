CREATE TABLE IF NOT EXISTS metric_points (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id     TEXT NOT NULL,
    run_id      TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    ts          TEXT NOT NULL,
    value       REAL NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id)
);

CREATE INDEX IF NOT EXISTS idx_metric_points_task_run ON metric_points(task_id, run_id);
CREATE INDEX IF NOT EXISTS idx_metric_points_ts ON metric_points(ts);
