CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY NOT NULL,
    user_id     TEXT NOT NULL,
    token       TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    expires_at  TEXT,
    ip          TEXT,
    user_agent  TEXT,
    FOREIGN KEY (user_id) REFERENCES users(id)
);
