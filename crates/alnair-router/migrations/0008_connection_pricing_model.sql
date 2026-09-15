-- Optional pricing lookup override per connection: relays often expose model ids
-- that differ from the catalog (e.g. `ocg/openai/gpt-5.6-luna`).

ALTER TABLE connections ADD COLUMN pricing_model TEXT;
