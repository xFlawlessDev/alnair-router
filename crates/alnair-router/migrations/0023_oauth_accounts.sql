-- OAuth accounts backing a connection.
--
-- A connection that authenticates with OAuth holds one or more of these instead
-- of a static key. The whole credential document travels encrypted through
-- `CredentialCipher` as a single JSON blob, so access token, refresh token and
-- the endpoints needed to refresh it stay together and are rotated as a unit.
--
-- `provider_key` names the endpoint preset the login used (`gitlab-duo`,
-- `google`, or `generic`); the client id/secret themselves live in the blob as
-- supplied by the operator. No first-party client identity is compiled in.

CREATE TABLE IF NOT EXISTS oauth_accounts (
    id            TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    label         TEXT NOT NULL,
    provider_key  TEXT NOT NULL,
    credential    TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    UNIQUE (connection_id, label)
);

CREATE INDEX IF NOT EXISTS idx_oauth_accounts_connection
    ON oauth_accounts (connection_id);
