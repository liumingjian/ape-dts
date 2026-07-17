-- Add per-Run metrics port column. NULL means no port has been allocated (legacy rows).
ALTER TABLE runs ADD COLUMN metrics_port INTEGER NULL;
