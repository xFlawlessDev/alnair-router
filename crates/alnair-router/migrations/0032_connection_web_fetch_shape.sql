-- Wire shape of a provider extract request body. `web_fetch_path` picks the
-- endpoint, but the endpoints disagree on how a URL is named:
--   target - `{ "url": ..., "format": ... }` (Firecrawl `/v1/scrape`)
--   urls   - `{ "urls": [...], "format": ... }` (Tavily `/extract`, Exa
--            `/contents`), where a bare `url` field is rejected with a 422.
--
-- NULL keeps the original `target` shape, so no existing connection changes
-- behaviour; a preset fills the shape its own endpoint expects.

ALTER TABLE connections ADD COLUMN web_fetch_shape TEXT;
