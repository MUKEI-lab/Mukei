-- V002__recovery_state.sql — crash-safe stream resume state
-- PRD REQ-STATE-01 / TRD §6.1
CREATE TABLE IF NOT EXISTS recovery_state (
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
    FOREIGN KEY (last_message_id) REFERENCES messages(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_recovery_conv ON recovery_state(conversation_id);
