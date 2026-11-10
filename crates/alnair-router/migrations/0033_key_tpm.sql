ALTER TABLE api_keys ADD COLUMN tokens_per_minute INTEGER CHECK (tokens_per_minute >= 0);
ALTER TABLE key_plans ADD COLUMN tokens_per_minute INTEGER CHECK (tokens_per_minute >= 0);
