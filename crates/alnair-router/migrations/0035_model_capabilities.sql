CREATE TABLE model_capability_catalog (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    catalog TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE model_capability_overrides (
    scope TEXT NOT NULL,
    target_id TEXT NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY (scope, target_id)
);
