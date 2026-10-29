-- Admit the ChatGPT/Codex wire family in provider_type.
--
-- SQLite cannot alter a CHECK constraint, so rebuild the connections table while
-- preserving child rows that reference it.

CREATE TEMP TABLE aliases_backup AS SELECT * FROM aliases;
CREATE TEMP TABLE connection_accounts_backup AS SELECT * FROM connection_accounts;
CREATE TEMP TABLE oauth_accounts_backup AS SELECT * FROM oauth_accounts;

CREATE TABLE connections_new (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL UNIQUE,
    provider_type      TEXT NOT NULL CHECK (provider_type IN ('openai-compatible', 'anthropic-native', 'command-code', 'codebuddy-intl', 'openai-codex')),
    base_url           TEXT NOT NULL,
    api_key            TEXT,
    custom_headers     TEXT NOT NULL DEFAULT '{}',
    enabled            INTEGER NOT NULL DEFAULT 1,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    connect_timeout_ms INTEGER,
    idle_timeout_ms    INTEGER,
    pricing_model      TEXT,
    provider_id        TEXT,
    cache_retention    TEXT NOT NULL DEFAULT 'none',
    auth_style         TEXT NOT NULL DEFAULT 'api_key',
    web_fetch_mode     TEXT NOT NULL DEFAULT 'local',
    web_fetch_path     TEXT
);

INSERT INTO connections_new
    (id, name, provider_type, base_url, api_key, custom_headers, enabled,
     created_at, updated_at, connect_timeout_ms, idle_timeout_ms, pricing_model,
     provider_id, cache_retention, auth_style, web_fetch_mode, web_fetch_path)
SELECT
    id, name, provider_type, base_url, api_key, custom_headers, enabled,
    created_at, updated_at, connect_timeout_ms, idle_timeout_ms, pricing_model,
    provider_id, cache_retention, auth_style, web_fetch_mode, web_fetch_path
FROM connections;

DROP TABLE connections;
ALTER TABLE connections_new RENAME TO connections;

INSERT INTO aliases SELECT * FROM aliases_backup;
INSERT INTO connection_accounts SELECT * FROM connection_accounts_backup;
INSERT INTO oauth_accounts SELECT * FROM oauth_accounts_backup;

DROP TABLE aliases_backup;
DROP TABLE connection_accounts_backup;
DROP TABLE oauth_accounts_backup;
