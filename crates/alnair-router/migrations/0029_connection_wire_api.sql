-- Wire API per connection for the OpenAI-compatible family.
--
-- `chat` posts to `{base_url}/chat/completions` and parses the chat chunk
-- shape; `responses` posts to `{base_url}/responses` and parses the Responses
-- event sequence instead. The flag is scoped to openai-compatible because the
-- other wire families each have exactly one contract.
--
-- Values mirror the provider layer's WireApi:
--   chat      — /chat/completions
--   responses — /responses

ALTER TABLE connections ADD COLUMN wire_api TEXT NOT NULL DEFAULT 'chat';
