CREATE TABLE IF NOT EXISTS system_hosts (
    id             TEXT PRIMARY KEY NOT NULL,
    hostname       TEXT NOT NULL,
    ip             TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'unknown',
    last_heartbeat TEXT,
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
