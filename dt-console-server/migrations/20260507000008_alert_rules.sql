CREATE TABLE IF NOT EXISTS alert_rules (
    id                TEXT PRIMARY KEY NOT NULL,
    name              TEXT NOT NULL,
    metric_name       TEXT NOT NULL,
    operator          TEXT NOT NULL CHECK (operator IN ('>', '<', '>=', '<=', '==')),
    threshold         REAL NOT NULL,
    severity          TEXT NOT NULL DEFAULT 'warning' CHECK (severity IN ('info', 'warning', 'critical')),
    dwell_secs        INTEGER NOT NULL DEFAULT 0,
    enabled           INTEGER NOT NULL DEFAULT 1,
    resource_group_id TEXT,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (resource_group_id) REFERENCES resource_groups(id)
);
