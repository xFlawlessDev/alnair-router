-- Reusable key plans ("set the rules once") and per-key rule overrides.

CREATE TABLE IF NOT EXISTS key_plans (
    id                    TEXT PRIMARY KEY,
    name                  TEXT NOT NULL UNIQUE,
    description           TEXT NOT NULL DEFAULT '',
    allowed_models        TEXT NOT NULL DEFAULT '[]',
    rate_limit_per_minute INTEGER,
    monthly_budget_usd    REAL,
    budget_mode           TEXT NOT NULL DEFAULT 'off',
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL
);

ALTER TABLE api_keys ADD COLUMN plan_id TEXT REFERENCES key_plans (id) ON DELETE SET NULL;
ALTER TABLE api_keys ADD COLUMN allowed_models TEXT;
