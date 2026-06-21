-- V004__branching.sql — branch graph and fork metadata
-- TRD §6.1 / BS v1.2 branching model
CREATE TABLE IF NOT EXISTS branches (
    id                      INTEGER PRIMARY KEY,
    external_id             TEXT NOT NULL UNIQUE,
    conversation_id         INTEGER NOT NULL,
    parent_branch_id        INTEGER,
    forked_from_message_id  INTEGER,
    title                   TEXT NOT NULL DEFAULT '',
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,
    is_active               INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0, 1)),
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
    FOREIGN KEY (parent_branch_id) REFERENCES branches(id) ON DELETE SET NULL,
    FOREIGN KEY (forked_from_message_id) REFERENCES messages(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_branches_conv ON branches(conversation_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_branches_parent ON branches(parent_branch_id);

UPDATE conversations
SET active_branch_id = COALESCE(active_branch_id, (
    SELECT id FROM branches b WHERE b.conversation_id = conversations.id AND b.is_active = 1 ORDER BY b.id DESC LIMIT 1
))
WHERE active_branch_id IS NULL;
