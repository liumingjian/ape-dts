CREATE TABLE IF NOT EXISTS alerts (
    id           TEXT PRIMARY KEY NOT NULL,
    task_id      TEXT,
    run_id       TEXT,
    rule_id      TEXT,
    metric_name  TEXT,
    operator     TEXT,
    threshold    REAL,
    severity     TEXT NOT NULL DEFAULT 'warning' CHECK (severity IN ('info', 'warning', 'critical')),
    value        REAL,
    status       TEXT NOT NULL DEFAULT 'firing' CHECK (status IN ('firing', 'recovered', 'cleared')),
    fired_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    recovered_at TEXT,
    cleared_at   TEXT,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
