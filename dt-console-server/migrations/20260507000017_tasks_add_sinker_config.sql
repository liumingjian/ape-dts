-- Add sinker_config column to tasks (was missing from initial schema).
ALTER TABLE tasks ADD COLUMN sinker_config TEXT NOT NULL DEFAULT '{}';
