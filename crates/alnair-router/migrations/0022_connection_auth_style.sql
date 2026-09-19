-- How the credential is presented to this upstream:
--   api_key — the provider's native style (x-api-key for Anthropic-native,
--             bearer for OpenAI-compatible).
--   bearer  — always Authorization: Bearer, even on Anthropic-native.
--
-- `bearer` is what OAuth/subscription session tokens (Claude Code, Codex,
-- GitHub Copilot) and most Anthropic-compatible relays require; an Anthropic
-- endpoint rejects a session token sent as x-api-key. Defaults to the existing
-- behaviour so no connection changes how it authenticates.

ALTER TABLE connections ADD COLUMN auth_style TEXT NOT NULL DEFAULT 'api_key';
