CREATE TABLE IF NOT EXISTS alarm_channels (
    id                TEXT PRIMARY KEY NOT NULL,
    name              TEXT NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('kafka', 'snmp')),
    config            TEXT NOT NULL DEFAULT '{}',
    enabled           INTEGER NOT NULL DEFAULT 1,
    resource_group_id TEXT,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (resource_group_id) REFERENCES resource_groups(id)
);
