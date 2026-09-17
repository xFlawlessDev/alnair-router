-- Admit the CodeBuddy Intl wire family in provider_type.
--
-- SQLite cannot alter a CHECK constraint, so rebuild the connections table while
-- preserving child rows that reference it.

CREATE TEMP TABLE aliases_backup AS SELECT * FROM aliases;
CREATE TEMP TABLE connection_accounts_backup AS SELECT * FROM connection_accounts;

CREATE TABLE connections_new (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL UNIQUE,
    provider_type      TEXT NOT NULL CHECK (provider_type IN ('openai-compatible', 'anthropic-native', 'command-code', 'codebuddy-intl')),
    base_url           TEXT NOT NULL,
    api_key            TEXT,
    custom_headers     TEXT NOT NULL DEFAULT '{}',
    enabled            INTEGER NOT NULL DEFAULT 1,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    connect_timeout_ms INTEGER,
    idle_timeout_ms    INTEGER,
    pricing_model      TEXT,
    provider_id        TEXT
);

INSERT INTO connections_new
    (id, name, provider_type, base_url, api_key, custom_headers, enabled,
     created_at, updated_at, connect_timeout_ms, idle_timeout_ms, pricing_model, provider_id)
SELECT
    id, name, provider_type, base_url, api_key, custom_headers, enabled,
    created_at, updated_at, connect_timeout_ms, idle_timeout_ms, pricing_model, provider_id
FROM connections;

DROP TABLE connections;
ALTER TABLE connections_new RENAME TO connections;

INSERT INTO aliases SELECT * FROM aliases_backup;
INSERT INTO connection_accounts SELECT * FROM connection_accounts_backup;

DROP TABLE aliases_backup;
DROP TABLE connection_accounts_backup;
