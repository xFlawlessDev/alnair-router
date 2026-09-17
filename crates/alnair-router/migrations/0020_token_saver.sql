-- Token-saver savings per usage row. The RTK and Headroom columns are measured
-- prompt reductions; the directive columns are estimates derived from the
-- completion, because a terser answer is only knowable after the fact.

ALTER TABLE usage_records ADD COLUMN saved_rtk_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN saved_headroom_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN saved_terse_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN saved_caveman_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN saved_ponytail_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN saved_cost_usd REAL NOT NULL DEFAULT 0;
