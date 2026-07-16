-- Android runtime projection extension schema.
-- Versioned independently from the canonical V001..V013 migrator so an older
-- core binary does not classify the database as a newer incompatible schema.

CREATE TABLE IF NOT EXISTS runtime_projection_schema (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    version    INTEGER NOT NULL,
    checksum   TEXT NOT NULL,
    applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runtime_projections (
    domain         TEXT NOT NULL,
    projection_key TEXT NOT NULL,
    payload_json   TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    PRIMARY KEY (domain, projection_key)
);

CREATE INDEX IF NOT EXISTS idx_runtime_projections_domain_updated
    ON runtime_projections(domain, updated_at DESC);
