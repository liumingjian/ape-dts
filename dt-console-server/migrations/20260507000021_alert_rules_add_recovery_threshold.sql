-- Add recovery_threshold column to alert_rules.
-- When set, a firing alert only transitions to recovered when the value
-- crosses back past the recovery threshold (not just the firing threshold).
ALTER TABLE alert_rules ADD COLUMN recovery_threshold REAL;
