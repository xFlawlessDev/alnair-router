-- Dashboard-managed setting overrides.
--
-- A single JSON row applies over the file/env configuration at startup and is
-- hot-applied when the admin saves the Settings page.

CREATE TABLE IF NOT EXISTS settings (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    data       TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
