CREATE TABLE IF NOT EXISTS control_logs (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id         TEXT NOT NULL,
    run_id          TEXT,
    action          TEXT NOT NULL,
    intent_or_result TEXT NOT NULL CHECK (intent_or_result IN ('intent', 'result')),
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (task_id) REFERENCES tasks(id)
);
