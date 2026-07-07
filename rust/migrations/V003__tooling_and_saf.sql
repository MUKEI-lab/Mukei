-- V003__tooling_and_saf.sql — immutable tool audit chain + SAF registry
-- TRD §6.1 / BS v1.2
CREATE TABLE IF NOT EXISTS tool_audit_log (
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
CREATE UNIQUE INDEX IF NOT EXISTS idx_tool_audit_tool_call ON tool_audit_log(tool_call_id);
CREATE INDEX IF NOT EXISTS idx_tool_audit_conv ON tool_audit_log(conversation_id, created_at);

CREATE TABLE IF NOT EXISTS saf_tokens (
    token_id             TEXT PRIMARY KEY,
    source               TEXT NOT NULL DEFAULT 'android-saf',
    user_facing_label    TEXT NOT NULL,
    target               TEXT NOT NULL,
    mime_type            TEXT NOT NULL,
    size_bytes           INTEGER NOT NULL DEFAULT 0,
    persistable          INTEGER NOT NULL DEFAULT 1 CHECK (persistable IN (0, 1)),
    revoked              INTEGER NOT NULL DEFAULT 0 CHECK (revoked IN (0, 1)),
    created_at           TEXT NOT NULL,
    last_used_at         TEXT,
    revoke_reason        TEXT,
    cache_rel_path       TEXT,
    content_sha256       TEXT
);
CREATE INDEX IF NOT EXISTS idx_saf_tokens_revoked ON saf_tokens(revoked, last_used_at);
