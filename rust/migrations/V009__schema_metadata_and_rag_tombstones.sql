-- V009__schema_metadata_and_rag_tombstones.sql
--
-- v0.8 static-review followups:
--   * Schema metadata (issue #6): a single-row `schema_metadata`
--     table that records the app_version that last applied a
--     migration. Combined with `migration_lock` (below) the boot
--     path can detect "this file was written by a newer Mukei than
--     the binary running" and refuse to start, instead of silently
--     accepting a schema it cannot reason about.
--   * Migration concurrency safety (issue #6): a `migration_lock`
--     table that holds a single row pinned by `id = 1`. Each
--     `apply_pending` call MUST either INSERT this row or refuse to
--     run. Two concurrent boot processes cannot double-apply because
--     the INSERT will fail with a unique-key violation.
--   * RAG delete audit (issue #11): a `document_tombstone` table
--     that records the moment a SAF-granted file is revoked. Vector
--     cleanup is best-effort after the SQL transaction commits, so
--     `cleanup_pending = 1` is set when the vector reverbs the
--     safest possible semantic — "deleted but vectors may still
--     live". On the next boot a reconciliation job drains the
--     column to 0 once the vector side has been physically shredded.
--
-- This migration is purely additive — it does NOT modify any
-- existing rows. Older releases keep working unchanged.

-- 1) Single-row schema metadata ---------------------------------------
CREATE TABLE IF NOT EXISTS schema_metadata (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    app_version     TEXT NOT NULL,
    last_migration  INTEGER NOT NULL,
    applied_at      TEXT NOT NULL
);

-- Seed the row if this migration runs on an existing DB so
-- the boot path always finds a row to read.
INSERT OR IGNORE INTO schema_metadata (id, app_version, last_migration, applied_at)
VALUES (1, '0.0.0-bootstrap', 8, COALESCE(
    (SELECT applied_at FROM migrations_applied WHERE version = 8),
    (SELECT applied_at FROM migrations_applied ORDER BY version DESC LIMIT 1),
    '1970-01-01T00:00:00Z'
));

UPDATE schema_metadata
SET last_migration = 9,
    applied_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id = 1;

-- 2) Migration concurrency lock ---------------------------------------
-- A second boot process attempting concurrent apply_pending against
-- the SAME encrypted DB will trip the unique-constraint insert below
-- and surface a clean typed error to the user-facing first-run UI
-- instead of producing a half-applied database.
CREATE TABLE IF NOT EXISTS migration_lock (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    holder      TEXT NOT NULL,       -- opaque boot-process tag
    acquired_at TEXT NOT NULL        -- RFC 3339 timestamp
);

INSERT OR IGNORE INTO migration_lock (id, holder, acquired_at)
VALUES (
    1,
    'bootstrap',                     -- overwritten by apply_pending
    '1970-01-01T00:00:00Z'
);

-- 3) RAG tombstone ledger ---------------------------------------------
CREATE TABLE IF NOT EXISTS document_tombstone (
    file_token     TEXT PRIMARY KEY,
    revoked_at     TEXT NOT NULL,
    reason         TEXT NOT NULL,    -- e.g. 'saf-revoke', 'manual-delete'
    chunks_deleted INTEGER NOT NULL DEFAULT 0,
    cleanup_pending INTEGER NOT NULL DEFAULT 1 CHECK (cleanup_pending IN (0, 1)),
    audited_event_id INTEGER         -- NULL = not yet audited
);

-- Index lets the boot reaper list pending cleanups quickly without
-- scanning rows that have already been reaped.
CREATE INDEX IF NOT EXISTS idx_document_tombstone_pending
    ON document_tombstone(cleanup_pending, revoked_at)
    WHERE cleanup_pending = 1;

-- Covering index for SAF prompt-on-reuse queries now that tombstone
-- is real: even after SQL chunks were deleted the boot path can
-- confirm the file_token is permanently retired.
CREATE INDEX IF NOT EXISTS idx_document_tombstone_token
    ON document_tombstone(file_token, cleanup_pending);
