-- Opt-in prompt caching per connection. Anthropic-native upstreams accept
-- `cache_control` breakpoints; OpenAI-compatible endpoints may reject the
-- extra field, so it stays off ('none') unless explicitly enabled.
--
-- Values mirror the provider layer's CacheRetention:
--   none  — no cache_control breakpoints
--   short — Anthropic's default 5-minute ephemeral cache
--   long  — 1-hour cache (requires the extended-cache-ttl beta header)

ALTER TABLE connections ADD COLUMN cache_retention TEXT NOT NULL DEFAULT 'none';
