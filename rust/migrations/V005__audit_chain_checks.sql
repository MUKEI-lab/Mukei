-- V005__audit_chain_checks.sql
-- Architect review GH #41 — strengthen the hash-chain invariants on
-- tool_audit_log so AuditLogReader::verify_chain (GH #19) can depend
-- on shape guarantees the SQLite engine enforces at INSERT time, not
-- just at read time.
--
-- SQLite cannot ALTER TABLE ADD CONSTRAINT, so we drop+recreate via
-- the standard "rename, recreate, copy, drop, rename back" idiom.

PRAGMA foreign_keys = OFF;

ALTER TABLE tool_audit_log RENAME TO tool_audit_log__pre_v005;

CREATE TABLE tool_audit_log (
    id                 INTEGER PRIMARY KEY,
    conversation_id    INTEGER,
    message_id         INTEGER,
    tool_call_id       TEXT    NOT NULL,
    tool_name          TEXT    NOT NULL,
    args_json          TEXT    NOT NULL,
    result_preview     TEXT    NOT NULL DEFAULT '',
    success            INTEGER NOT NULL CHECK (success IN (0, 1)),
    duration_ms        INTEGER NOT NULL DEFAULT 0 CHECK (duration_ms >= 0),
    error_code         TEXT,
    -- GH #41: fingerprints are SHA-256 hex (64 chars). Enforce at the
    -- DB layer so a row that bypasses AuditLogWriter cannot poison the
    -- chain.
    fingerprint_sha256 TEXT    NOT NULL CHECK (length(fingerprint_sha256) = 64),
    previous_hash      TEXT             CHECK (previous_hash IS NULL OR length(previous_hash) = 64),
    entry_hash         TEXT    NOT NULL CHECK (length(entry_hash) = 64),
    created_at         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE SET NULL,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE SET NULL
);

INSERT INTO tool_audit_log
SELECT * FROM tool_audit_log__pre_v005;

DROP TABLE tool_audit_log__pre_v005;

CREATE UNIQUE INDEX IF NOT EXISTS idx_tool_audit_tool_call
    ON tool_audit_log(tool_call_id);
CREATE INDEX IF NOT EXISTS idx_tool_audit_conv
    ON tool_audit_log(conversation_id, created_at);

PRAGMA foreign_keys = ON;
