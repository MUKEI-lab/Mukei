-- V008__settings_and_secret_refs.sql — split preferences from secret handles
-- Normal settings may be exported/synced later. Secret material itself
-- must never be written here; secret_refs stores only opaque secure-store
-- handles such as Android Keystore aliases/blob ids.

CREATE TABLE IF NOT EXISTS preferences (
    key          TEXT PRIMARY KEY,
    value_json   TEXT NOT NULL,
    value_type   TEXT NOT NULL CHECK (value_type IN ('bool', 'integer', 'string')),
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS secret_refs (
    slot         TEXT PRIMARY KEY,
    provider     TEXT NOT NULL,
    storage_key  TEXT NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_secret_refs_provider ON secret_refs(provider);
