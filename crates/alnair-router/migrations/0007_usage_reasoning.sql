-- Reasoning tokens are part of completion_tokens but billed at their own rate;
-- store them so usage rows explain the cost.

ALTER TABLE usage_records ADD COLUMN reasoning_tokens INTEGER NOT NULL DEFAULT 0;
