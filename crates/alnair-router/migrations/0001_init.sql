-- alnair-router initial schema.

CREATE TABLE IF NOT EXISTS connections (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL UNIQUE,
    provider_type  TEXT NOT NULL CHECK (provider_type IN ('openai-compatible', 'anthropic-native')),
    base_url       TEXT NOT NULL,
    api_key        TEXT,
    custom_headers TEXT NOT NULL DEFAULT '{}',
    enabled        INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS aliases (
    id            TEXT PRIMARY KEY,
    prefix        TEXT NOT NULL UNIQUE,
    connection_id TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    model_override TEXT,
    enabled       INTEGER NOT NULL DEFAULT 1,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_aliases_connection ON aliases (connection_id);

CREATE TABLE IF NOT EXISTS combos (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS combo_entries (
    id       TEXT PRIMARY KEY,
    combo_id TEXT NOT NULL REFERENCES combos (id) ON DELETE CASCADE,
    model_ref TEXT NOT NULL,
    position INTEGER NOT NULL,
    enabled  INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_combo_entries_combo ON combo_entries (combo_id, position);

CREATE TABLE IF NOT EXISTS api_keys (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    key_hash     TEXT NOT NULL UNIQUE,
    prefix       TEXT NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 1,
    created_at   TEXT NOT NULL,
    last_used_at TEXT
);

CREATE TABLE IF NOT EXISTS usage_records (
    id                 TEXT PRIMARY KEY,
    created_at         TEXT NOT NULL,
    api_key_id         TEXT REFERENCES api_keys (id) ON DELETE SET NULL,
    requested_model    TEXT NOT NULL,
    resolved_provider  TEXT,
    resolved_model     TEXT,
    attempt            INTEGER NOT NULL DEFAULT 1,
    status             TEXT NOT NULL,
    prompt_tokens      INTEGER NOT NULL DEFAULT 0,
    completion_tokens  INTEGER NOT NULL DEFAULT 0,
    cached_tokens      INTEGER NOT NULL DEFAULT 0,
    cost_usd           REAL NOT NULL DEFAULT 0,
    latency_ms         INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_usage_created ON usage_records (created_at);
CREATE INDEX IF NOT EXISTS idx_usage_model ON usage_records (requested_model);
