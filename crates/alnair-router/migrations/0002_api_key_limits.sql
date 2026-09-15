-- Per-key limits and budgets.

ALTER TABLE api_keys ADD COLUMN rate_limit_per_minute INTEGER;
ALTER TABLE api_keys ADD COLUMN monthly_budget_usd REAL;
ALTER TABLE api_keys ADD COLUMN budget_mode TEXT NOT NULL DEFAULT 'off';
