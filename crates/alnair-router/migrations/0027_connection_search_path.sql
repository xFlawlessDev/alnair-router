-- Path appended to `base_url` for a search request. NULL means the default
-- `/search`, which is what most providers expose. Providers that serve search
-- elsewhere (for example Brave's `/web/search`) set it per connection.
ALTER TABLE connections ADD COLUMN search_path TEXT;
