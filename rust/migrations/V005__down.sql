-- V005__down.sql — Architect review GH #25 + GH #41 companion.
-- Rolls back the strengthened hash-chain CHECK constraints. The DDL is
-- the same shape as the V003 original.
PRAGMA foreign_keys = OFF;
ALTER TABLE tool_audit_log RENAME TO tool_audit_log__v005;
CREATE TABLE tool_audit_log (
    id                 INTEGER PRIMARY KEY,
    conversation_id    INTEGER,
    message_id         INTEGER,
    tool_call_id       TEXT NOT NULL,
    tool_name          TEXT NOT NULL,
    args_json          TEXT NOT NULL,
    result_preview     TEXT NOT NULL DEFAULT '',
    success            INTEGER NOT NULL CHECK (success IN (0, 1)),
    duration_ms        INTEGER NOT NULL DEFAULT 0,
    error_code         TEXT,
    fingerprint_sha256 TEXT NOT NULL,
    previous_hash      TEXT,
    entry_hash         TEXT NOT NULL,
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE SET NULL,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE SET NULL
);
INSERT INTO tool_audit_log SELECT * FROM tool_audit_log__v005;
DROP TABLE tool_audit_log__v005;
CREATE UNIQUE INDEX IF NOT EXISTS idx_tool_audit_tool_call ON tool_audit_log(tool_call_id);
CREATE INDEX IF NOT EXISTS idx_tool_audit_conv ON tool_audit_log(conversation_id, created_at);
PRAGMA foreign_keys = ON;
