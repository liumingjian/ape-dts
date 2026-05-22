-- Add resource_group_id to users for RG-scoped access.
-- NULL means "all resource groups" (admin default).
-- Non-null means the user is scoped to that specific RG.
ALTER TABLE users ADD COLUMN resource_group_id TEXT DEFAULT NULL;
