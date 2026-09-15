-- Track which built-in provider preset a connection came from, so the
-- dashboard can show a badge and count usage per provider.

ALTER TABLE connections ADD COLUMN provider_id TEXT;
