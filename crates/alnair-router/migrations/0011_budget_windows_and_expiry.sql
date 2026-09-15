-- Multi-window budgets (daily/weekly/monthly) and optional expiry for keys and plans.

ALTER TABLE api_keys ADD COLUMN daily_budget_usd REAL;
ALTER TABLE api_keys ADD COLUMN weekly_budget_usd REAL;
ALTER TABLE api_keys ADD COLUMN expires_at TEXT;

ALTER TABLE key_plans ADD COLUMN daily_budget_usd REAL;
ALTER TABLE key_plans ADD COLUMN weekly_budget_usd REAL;
ALTER TABLE key_plans ADD COLUMN expires_at TEXT;
