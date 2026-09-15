-- Lifetime (no reset) spend budget for keys and plans.

ALTER TABLE api_keys ADD COLUMN lifetime_budget_usd REAL;
ALTER TABLE key_plans ADD COLUMN lifetime_budget_usd REAL;
