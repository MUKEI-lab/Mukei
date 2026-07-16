-- V014__runtime_projections.sql
-- Durable authoritative projections for the Android runtime boundary.

CREATE TABLE IF NOT EXISTS runtime_projections (
    domain       TEXT NOT NULL,
    projection_key TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    PRIMARY KEY (domain, projection_key)
);

CREATE INDEX IF NOT EXISTS idx_runtime_projections_domain_updated
    ON runtime_projections(domain, updated_at DESC);

UPDATE schema_metadata
SET last_migration = 14,
    applied_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id = 1;
