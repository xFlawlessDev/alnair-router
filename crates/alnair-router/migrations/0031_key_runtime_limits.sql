ALTER TABLE api_keys ADD COLUMN max_concurrent INTEGER CHECK (max_concurrent >= 0);
ALTER TABLE key_plans ADD COLUMN max_concurrent INTEGER CHECK (max_concurrent >= 0);
