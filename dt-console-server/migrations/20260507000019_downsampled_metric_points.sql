CREATE TABLE IF NOT EXISTS downsampled_metric_points (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id     TEXT NOT NULL,
    run_id      TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    bucket_ts   TEXT NOT NULL,
    bucket_secs INTEGER NOT NULL,
    value_mean  REAL NOT NULL,
    value_min   REAL NOT NULL,
    value_max   REAL NOT NULL,
    sample_count INTEGER NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id)
);

CREATE INDEX IF NOT EXISTS idx_downsampled_task_run_metric_ts
    ON downsampled_metric_points(task_id, run_id, metric_name, bucket_ts);
