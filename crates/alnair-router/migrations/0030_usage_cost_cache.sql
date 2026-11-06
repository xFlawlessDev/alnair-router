-- Cache share of the input cost, per usage row.
--
-- `cost_input_usd` already includes the cached tokens, billed at the cache-read
-- (or cache-write) rate rather than the input rate. Without this column the
-- dashboard can only show one "Input" figure, which misreads badly on a
-- cache-heavy workload: a 99%-cached prompt looks like it was billed at the
-- full input rate. Storing the cache portion lets the breakdown show an
-- ordinary "Input" row and a separate "Cache read" row that add up to the
-- stored total.
--
-- The column defaults to 0 so rows written before this migration keep reading
-- exactly as they did.

ALTER TABLE usage_records ADD COLUMN cost_cache_usd REAL NOT NULL DEFAULT 0;
