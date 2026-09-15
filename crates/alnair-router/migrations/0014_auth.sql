-- Single-owner admin password plus its opaque token sessions.
--
-- The dashboard signs in with a password only (no username). Refresh tokens
-- rotate on every use; presenting a rotated token revokes the whole family,
-- which is how token theft is detected.

CREATE TABLE IF NOT EXISTS auth (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    password_hash TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS auth_sessions (
    id                  TEXT PRIMARY KEY,
    token_hash          TEXT NOT NULL UNIQUE,
    kind                TEXT NOT NULL CHECK (kind IN ('access', 'refresh')),
    family_id           TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    expires_at          TEXT NOT NULL,
    absolute_expires_at TEXT NOT NULL,
    rotated_at          TEXT,
    revoked_at          TEXT
);

CREATE INDEX IF NOT EXISTS idx_auth_sessions_family ON auth_sessions (family_id);
