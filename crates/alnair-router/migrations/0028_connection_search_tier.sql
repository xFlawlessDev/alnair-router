-- Search tier for `/v1/search` fallback. NULL means the connection is not a
-- search fallback. Lower tiers are tried first; connections that share a tier
-- are rotated round-robin.
ALTER TABLE connections ADD COLUMN search_tier INTEGER;
