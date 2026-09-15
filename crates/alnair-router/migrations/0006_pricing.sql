-- Model pricing: user overrides plus the last crawled catalog.

CREATE TABLE IF NOT EXISTS model_prices (
    model                       TEXT NOT NULL,
    input_per_million_usd       REAL NOT NULL,
    output_per_million_usd      REAL NOT NULL,
    cache_read_per_million_usd  REAL,
    cache_write_per_million_usd REAL,
    reasoning_per_million_usd   REAL,
    source                      TEXT NOT NULL CHECK (source IN ('sync', 'override')),
    updated_at                  TEXT NOT NULL,
    PRIMARY KEY (model, source)
);

CREATE INDEX IF NOT EXISTS idx_model_prices_model ON model_prices (model);

CREATE TABLE IF NOT EXISTS pricing_sync_runs (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    source      TEXT NOT NULL,
    synced_at   TEXT NOT NULL,
    model_count INTEGER NOT NULL
);
