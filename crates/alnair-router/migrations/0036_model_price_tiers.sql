-- Conditional model pricing: an ordered list of rate tiers per row.
--
-- Each entry is a JSON object with `context_above`, `start_minute_utc`,
-- `end_minute_utc` (all nullable) plus the five per-million rates. An empty
-- array means the row's base rates apply unconditionally, which is why the
-- column defaults to `[]` and existing rows keep their current prices.
ALTER TABLE model_prices ADD COLUMN tiers TEXT NOT NULL DEFAULT '[]';
