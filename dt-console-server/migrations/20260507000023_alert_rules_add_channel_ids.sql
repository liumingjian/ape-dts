-- Add channel_ids column to alert_rules (JSON array of alarm_channel IDs).
ALTER TABLE alert_rules ADD COLUMN channel_ids TEXT NOT NULL DEFAULT '[]';
