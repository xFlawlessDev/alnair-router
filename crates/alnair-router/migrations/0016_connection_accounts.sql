-- Extra API keys per connection.
--
-- Requests rotate across the connection's primary key and these accounts, so
-- one provider endpoint can use several keys / quota buckets without creating
-- extra connections.

CREATE TABLE IF NOT EXISTS connection_accounts (
    id            TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    label         TEXT NOT NULL,
    api_key       TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    UNIQUE (connection_id, label)
);

CREATE INDEX IF NOT EXISTS idx_connection_accounts_connection
    ON connection_accounts (connection_id);
