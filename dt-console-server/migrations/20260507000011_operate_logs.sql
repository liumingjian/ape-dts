CREATE TABLE IF NOT EXISTS operate_logs (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    actor      TEXT NOT NULL,
    action     TEXT NOT NULL,
    result     TEXT NOT NULL DEFAULT 'success',
    target     TEXT,
    details    TEXT,
    ip         TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
