-- Per-window token spend limits (prompt + completion tokens) for keys and plans.

ALTER TABLE api_keys ADD COLUMN daily_token_limit INTEGER;
ALTER TABLE api_keys ADD COLUMN weekly_token_limit INTEGER;
ALTER TABLE api_keys ADD COLUMN monthly_token_limit INTEGER;
ALTER TABLE api_keys ADD COLUMN lifetime_token_limit INTEGER;

ALTER TABLE key_plans ADD COLUMN daily_token_limit INTEGER;
ALTER TABLE key_plans ADD COLUMN weekly_token_limit INTEGER;
ALTER TABLE key_plans ADD COLUMN monthly_token_limit INTEGER;
ALTER TABLE key_plans ADD COLUMN lifetime_token_limit INTEGER;
