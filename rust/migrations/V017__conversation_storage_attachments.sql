-- Durable conversation-level references to Universal Storage files.
--
-- Conversation lifecycle is currently authoritative in the encrypted runtime
-- projection, while file identity is authoritative in storage_nodes. Therefore
-- conversation_id is stored as the stable protocol UUID rather than inventing a
-- shadow FK to the legacy conversations table. node_id retains a hard FK to the
-- logical storage tree.

CREATE TABLE IF NOT EXISTS conversation_storage_attachments (
    attachment_id   TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL CHECK (length(trim(conversation_id)) > 0),
    node_id         TEXT NOT NULL,
    state           TEXT NOT NULL CHECK (state IN ('active', 'removed')),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    removed_at      TEXT,
    FOREIGN KEY (node_id) REFERENCES storage_nodes(node_id) ON DELETE RESTRICT,
    UNIQUE (conversation_id, node_id),
    CHECK ((state = 'removed' AND removed_at IS NOT NULL) OR state != 'removed')
);

CREATE INDEX IF NOT EXISTS conversation_storage_attachments_lookup
ON conversation_storage_attachments(conversation_id, state, created_at, attachment_id);

-- Attachment identity and ownership never mutate. Re-attachment reactivates the
-- same row; removing an attachment only changes lifecycle fields.
CREATE TRIGGER IF NOT EXISTS conversation_storage_attachment_identity_immutable
BEFORE UPDATE OF attachment_id, conversation_id, node_id ON conversation_storage_attachments
WHEN NEW.attachment_id IS NOT OLD.attachment_id
  OR NEW.conversation_id IS NOT OLD.conversation_id
  OR NEW.node_id IS NOT OLD.node_id
BEGIN
    SELECT RAISE(ABORT, 'conversation storage attachment identity is immutable');
END;

PRAGMA user_version = 17;
