CREATE TABLE IF NOT EXISTS licenses (
    id                  TEXT PRIMARY KEY NOT NULL,
    sku                 TEXT NOT NULL DEFAULT '',
    max_tasks           INTEGER NOT NULL DEFAULT 0,
    expire_at           TEXT,
    activated_at        TEXT,
    activation_code_hash TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
