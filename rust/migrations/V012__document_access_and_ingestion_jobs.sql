-- V012__document_access_and_ingestion_jobs.sql
-- Persist the actual Android URI permission outcome and the durable
-- document-ingestion queue separately from the raw SAF token registry.

ALTER TABLE saf_tokens
    ADD COLUMN os_permission_state TEXT NOT NULL DEFAULT 'unknown'
    CHECK (os_permission_state IN ('unknown', 'persisted', 'transient', 'not_required', 'failed'));

CREATE TABLE IF NOT EXISTS document_ingestion_jobs (
    document_id        TEXT PRIMARY KEY,
    token_id           TEXT NOT NULL UNIQUE,
    state              TEXT NOT NULL CHECK (
        state IN ('queued', 'reading', 'chunking', 'embedding', 'committing',
                  'completed', 'failed', 'cancelled', 'waiting_for_embedder')
    ),
    progress_percent   INTEGER NOT NULL DEFAULT 0 CHECK (progress_percent BETWEEN 0 AND 100),
    chunk_count        INTEGER NOT NULL DEFAULT 0 CHECK (chunk_count >= 0),
    retryable          INTEGER NOT NULL DEFAULT 1 CHECK (retryable IN (0, 1)),
    last_error         TEXT,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    FOREIGN KEY(token_id) REFERENCES saf_tokens(token_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_document_ingestion_state_updated
    ON document_ingestion_jobs(state, updated_at);

UPDATE schema_metadata
SET last_migration = 12,
    applied_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id = 1;
