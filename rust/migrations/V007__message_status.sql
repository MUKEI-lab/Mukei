-- V007__message_status.sql — durable turn lifecycle state
-- Adds explicit status/update metadata without rewriting historical rows.

ALTER TABLE messages
    ADD COLUMN status TEXT NOT NULL DEFAULT 'completed'
    CHECK (status IN ('pending', 'streaming', 'completed', 'failed', 'cancelled'));

ALTER TABLE messages
    ADD COLUMN updated_at TEXT;

UPDATE messages
SET updated_at = created_at
WHERE updated_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_messages_status
    ON messages(conversation_id, status, updated_at, id);
