-- Where `/v1/web/fetch` gets its content from:
--   local  — the router fetches the URL itself, behind the SSRF guard.
--   provider — the router forwards the URL to the connection's own extract
--              endpoint instead.
--
-- `local` stays the default so no existing connection changes behaviour. A
-- provider connection is how an operator reaches pages that need JavaScript
-- rendering, which a plain HTTP fetch cannot produce. The path is configurable
-- because extract endpoints are not standardised: Firecrawl serves
-- `/v1/scrape`, Tavily `/extract`, Jina `/` and Exa `/contents`.

ALTER TABLE connections ADD COLUMN web_fetch_mode TEXT NOT NULL DEFAULT 'local';
ALTER TABLE connections ADD COLUMN web_fetch_path TEXT;
