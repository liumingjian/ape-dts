CREATE TABLE IF NOT EXISTS alarm_templates (
    id               TEXT PRIMARY KEY NOT NULL,
    name             TEXT NOT NULL,
    subject_template TEXT NOT NULL DEFAULT '',
    body_template    TEXT NOT NULL DEFAULT '',
    severity_mapping TEXT NOT NULL DEFAULT '{}',
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
