CREATE TABLE IF NOT EXISTS global_params (
    id         TEXT PRIMARY KEY NOT NULL,
    key        TEXT NOT NULL UNIQUE,
    value      TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
