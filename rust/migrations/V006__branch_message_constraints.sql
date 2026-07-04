-- V006__branch_message_constraints.sql
-- Architect review GH #41 — enforce branch/message relationship
-- constraints that could not exist before V004 introduced branches.

PRAGMA foreign_keys = OFF;

CREATE UNIQUE INDEX IF NOT EXISTS idx_branches_id_conversation
    ON branches(id, conversation_id);

ALTER TABLE messages RENAME TO messages__pre_v006;

CREATE TABLE messages (
    id                  INTEGER PRIMARY KEY,
    external_id         TEXT NOT NULL UNIQUE,
    conversation_id     INTEGER NOT NULL,
    role                TEXT NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool', 'red_team')),
    content             TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    branch_id           INTEGER,
    parent_message_id   INTEGER,
    tool_call_id        TEXT,
    tool_name           TEXT,
    token_count         INTEGER NOT NULL DEFAULT 0,
    deleted             INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
    FOREIGN KEY (branch_id) REFERENCES branches(id) ON DELETE SET NULL,
    FOREIGN KEY (branch_id, conversation_id) REFERENCES branches(id, conversation_id),
    FOREIGN KEY (parent_message_id) REFERENCES messages(id) ON DELETE SET NULL
);

INSERT INTO messages (
    id,
    external_id,
    conversation_id,
    role,
    content,
    created_at,
    branch_id,
    parent_message_id,
    tool_call_id,
    tool_name,
    token_count,
    deleted
)
SELECT
    id,
    external_id,
    conversation_id,
    role,
    content,
    created_at,
    CASE
        WHEN branch_id IS NULL THEN NULL
        WHEN EXISTS (
            SELECT 1 FROM branches b
            WHERE b.id = messages__pre_v006.branch_id
              AND b.conversation_id = messages__pre_v006.conversation_id
        ) THEN branch_id
        ELSE NULL
    END,
    parent_message_id,
    tool_call_id,
    tool_name,
    token_count,
    deleted
FROM messages__pre_v006;

DROP TABLE messages__pre_v006;

CREATE INDEX IF NOT EXISTS idx_messages_conv_created ON messages(conversation_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_messages_branch ON messages(conversation_id, branch_id, id);
CREATE INDEX IF NOT EXISTS idx_messages_parent ON messages(parent_message_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_branch_id_pair
    ON messages(id, branch_id)
    WHERE branch_id IS NOT NULL;

ALTER TABLE recovery_state RENAME TO recovery_state__pre_v006;

CREATE TABLE recovery_state (
    id                    INTEGER PRIMARY KEY CHECK (id = 1),
    conversation_id       INTEGER NOT NULL,
    branch_id             INTEGER,
    last_message_id       INTEGER NOT NULL,
    prompt_snapshot       TEXT NOT NULL,
    generated_prefix      TEXT NOT NULL DEFAULT '',
    last_token_count      INTEGER NOT NULL DEFAULT 0,
    kv_cache_fingerprint  TEXT NOT NULL,
    model_fingerprint     TEXT,
    watchdog_fingerprint  TEXT,
    resumed_after_kill    INTEGER NOT NULL DEFAULT 0 CHECK (resumed_after_kill IN (0, 1)),
    updated_at            TEXT NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
    FOREIGN KEY (branch_id) REFERENCES branches(id) ON DELETE SET NULL,
    FOREIGN KEY (branch_id, conversation_id) REFERENCES branches(id, conversation_id),
    FOREIGN KEY (last_message_id) REFERENCES messages(id) ON DELETE CASCADE
);

INSERT INTO recovery_state (
    id,
    conversation_id,
    branch_id,
    last_message_id,
    prompt_snapshot,
    generated_prefix,
    last_token_count,
    kv_cache_fingerprint,
    model_fingerprint,
    watchdog_fingerprint,
    resumed_after_kill,
    updated_at
)
SELECT
    id,
    conversation_id,
    CASE
        WHEN branch_id IS NULL THEN NULL
        WHEN EXISTS (
            SELECT 1 FROM branches b
            WHERE b.id = recovery_state__pre_v006.branch_id
              AND b.conversation_id = recovery_state__pre_v006.conversation_id
        ) THEN branch_id
        ELSE NULL
    END,
    last_message_id,
    prompt_snapshot,
    generated_prefix,
    last_token_count,
    kv_cache_fingerprint,
    model_fingerprint,
    watchdog_fingerprint,
    resumed_after_kill,
    updated_at
FROM recovery_state__pre_v006;

DROP TABLE recovery_state__pre_v006;

CREATE INDEX IF NOT EXISTS idx_recovery_conv ON recovery_state(conversation_id);

UPDATE branches
SET is_active = 0
WHERE is_active = 1
  AND id NOT IN (
      SELECT MAX(id)
      FROM branches
      WHERE is_active = 1
      GROUP BY conversation_id
  );

CREATE UNIQUE INDEX IF NOT EXISTS idx_branches_one_active_per_conversation
    ON branches(conversation_id)
    WHERE is_active = 1;

PRAGMA foreign_keys = ON;
