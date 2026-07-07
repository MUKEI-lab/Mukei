-- V001__schema.sql — core conversation/message/chunk schema
-- TRD §6.1 / BS v1.2 baseline
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS migrations_applied (
    version        INTEGER PRIMARY KEY,
    name           TEXT NOT NULL UNIQUE,
    applied_at     TEXT NOT NULL,
    checksum       TEXT,
    execution_ms   INTEGER,
    success        INTEGER NOT NULL DEFAULT 1 CHECK (success IN (0, 1))
);

CREATE TABLE IF NOT EXISTS conversations (
    id               INTEGER PRIMARY KEY,
    external_id      TEXT NOT NULL UNIQUE,
    title            TEXT NOT NULL DEFAULT '',
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    archived         INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    active_branch_id INTEGER
);
CREATE INDEX IF NOT EXISTS idx_conversations_updated_at ON conversations(updated_at DESC);

CREATE TABLE IF NOT EXISTS messages (
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
    FOREIGN KEY (parent_message_id) REFERENCES messages(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_messages_conv_created ON messages(conversation_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_messages_branch ON messages(conversation_id, branch_id, id);
CREATE INDEX IF NOT EXISTS idx_messages_parent ON messages(parent_message_id);

CREATE TABLE IF NOT EXISTS chunks (
    id                INTEGER PRIMARY KEY,
    chunk_uuid        TEXT NOT NULL UNIQUE,
    conversation_id   INTEGER,
    message_id        INTEGER,
    file_token        TEXT,
    ordinal           INTEGER NOT NULL DEFAULT 0,
    sha256            TEXT NOT NULL,
    token_count       INTEGER NOT NULL DEFAULT 0,
    embedding_dim     INTEGER,
    content           TEXT NOT NULL,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_chunks_message ON chunks(message_id, ordinal);
CREATE INDEX IF NOT EXISTS idx_chunks_file_token ON chunks(file_token);

DROP TABLE IF EXISTS config;
