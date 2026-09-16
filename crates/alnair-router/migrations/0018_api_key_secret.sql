-- Encrypted router-issued key secret, kept so the dashboard can reveal it.
--
-- Nullable: keys minted before this migration never had a recoverable secret,
-- and keys created while `server.store_key_secrets` is false deliberately keep
-- none. The value is AES-256-GCM ciphertext (`enc:v1:` marker), so it is never
-- usable straight from the database file.
ALTER TABLE api_keys ADD COLUMN secret_enc TEXT;
