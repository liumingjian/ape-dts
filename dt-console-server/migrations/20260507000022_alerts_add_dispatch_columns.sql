-- Add dispatch and lifecycle columns to alerts.
-- silenced: set when global silence window is active (alert recorded but not dispatched).
-- delivered_at: timestamp when AlarmDispatcher successfully delivered the alert.
-- cleared_by: user who cleared the alert (from session).
-- last_error: last dispatch error message (for dead-letter tracking).
ALTER TABLE alerts ADD COLUMN silenced INTEGER NOT NULL DEFAULT 0;
ALTER TABLE alerts ADD COLUMN delivered_at TEXT;
ALTER TABLE alerts ADD COLUMN cleared_by TEXT;
ALTER TABLE alerts ADD COLUMN last_error TEXT;
