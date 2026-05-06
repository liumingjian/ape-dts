CREATE TABLE IF NOT EXISTS runs (
    id           TEXT PRIMARY KEY NOT NULL,
    task_id      TEXT NOT NULL,
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
    FOREIGN KEY (task_id) REFERENCES tasks(id)
);
