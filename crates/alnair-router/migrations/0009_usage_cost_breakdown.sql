-- Cost breakdown per usage row: input, output and the reasoning premium are
-- stored separately so the dashboard can explain the total.

ALTER TABLE usage_records ADD COLUMN cost_input_usd REAL NOT NULL DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN cost_output_usd REAL NOT NULL DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN cost_reasoning_usd REAL NOT NULL DEFAULT 0;
