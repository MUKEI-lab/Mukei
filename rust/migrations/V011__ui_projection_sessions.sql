-- V011__ui_projection_sessions.sql
-- Persistent, versioned QML session and draft state. Domain truth remains in
-- conversations/messages; these tables only preserve safe UI restoration data.

CREATE TABLE IF NOT EXISTS ui_session_state (
    profile_id                  TEXT PRIMARY KEY,
    schema_version              INTEGER NOT NULL,
    active_route                TEXT NOT NULL DEFAULT 'boot',
    active_conversation_id      TEXT,
    active_branch_id            TEXT,
    timeline_anchor_message_id  TEXT,
    selected_model_id           TEXT,
    payload_json                TEXT NOT NULL DEFAULT '{}',
    updated_at                  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ui_drafts (
    conversation_id       TEXT NOT NULL,
    branch_id             TEXT NOT NULL,
    text                  TEXT NOT NULL DEFAULT '',
    cursor_position       INTEGER NOT NULL DEFAULT 0 CHECK (cursor_position >= 0),
    attachment_refs_json  TEXT NOT NULL DEFAULT '[]',
    updated_at            TEXT NOT NULL,
    PRIMARY KEY (conversation_id, branch_id)
);

CREATE INDEX IF NOT EXISTS idx_ui_drafts_updated_at
    ON ui_drafts(updated_at DESC);

UPDATE schema_metadata
SET last_migration = 11,
    applied_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id = 1;
