-- V010__reliability_hardening.sql
-- Durable cleanup/retry metadata and download reservations.

ALTER TABLE document_tombstone
    ADD COLUMN chunk_ids_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE document_tombstone
    ADD COLUMN cleanup_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE document_tombstone
    ADD COLUMN last_error TEXT;
ALTER TABLE document_tombstone
    ADD COLUMN updated_at TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z';

UPDATE document_tombstone
SET updated_at = CASE
    WHEN updated_at = '1970-01-01T00:00:00Z' THEN revoked_at
    ELSE updated_at
END;

CREATE TABLE IF NOT EXISTS download_jobs (
    job_id              TEXT PRIMARY KEY,
    model_id            TEXT,
    destination_token   TEXT NOT NULL,
    destination_path    TEXT NOT NULL,
    expected_sha256     TEXT NOT NULL,
    expected_bytes      INTEGER,
    bytes_downloaded    INTEGER NOT NULL DEFAULT 0,
    status              TEXT NOT NULL CHECK (
        status IN ('queued', 'downloading', 'completed', 'failed', 'cancelled')
    ),
    last_error_code     TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_download_jobs_status_updated
    ON download_jobs(status, updated_at);

CREATE TABLE IF NOT EXISTS storage_reservations (
    reservation_id      TEXT PRIMARY KEY,
    job_id              TEXT NOT NULL UNIQUE,
    storage_class       TEXT NOT NULL CHECK (storage_class IN ('model')),
    reserved_bytes      INTEGER NOT NULL CHECK (reserved_bytes >= 0),
    created_at          TEXT NOT NULL,
    expires_at          TEXT NOT NULL,
    FOREIGN KEY(job_id) REFERENCES download_jobs(job_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_storage_reservations_expiry
    ON storage_reservations(expires_at);

UPDATE schema_metadata
SET last_migration = 10,
    applied_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id = 1;
