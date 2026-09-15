-- Per-connection upstream timeouts (milliseconds). NULL inherits the global
-- `router.connect_timeout_ms` / `router.idle_timeout_ms`.

ALTER TABLE connections ADD COLUMN connect_timeout_ms INTEGER;
ALTER TABLE connections ADD COLUMN idle_timeout_ms INTEGER;
