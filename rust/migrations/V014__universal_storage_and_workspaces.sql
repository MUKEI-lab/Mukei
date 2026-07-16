-- Universal Storage and isolated per-chat workspaces.
--
-- User-facing directory entries are logical SQL rows. File bytes live in the
-- encrypted immutable object store and are addressed only by opaque object IDs.
-- This migration is append-only and must never be edited after release.

CREATE TABLE IF NOT EXISTS storage_schema_metadata (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version      INTEGER NOT NULL CHECK (schema_version >= 1),
    file_policy_version INTEGER NOT NULL CHECK (file_policy_version >= 1),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

INSERT OR IGNORE INTO storage_schema_metadata (
    id, schema_version, file_policy_version, created_at, updated_at
) VALUES (1, 1, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP);

CREATE TABLE IF NOT EXISTS storage_scopes (
    scope_id       TEXT PRIMARY KEY,
    scope_type     TEXT NOT NULL CHECK (scope_type IN ('universal', 'workspace')),
    owner_chat_id  TEXT,
    root_node_id   TEXT NOT NULL,
    display_name   TEXT NOT NULL,
    state          TEXT NOT NULL CHECK (
        state IN ('active', 'trashed', 'deleting', 'deleted')
    ),
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    CHECK (
        (scope_type = 'universal' AND owner_chat_id IS NULL)
        OR
        (scope_type = 'workspace' AND owner_chat_id IS NOT NULL AND length(trim(owner_chat_id)) > 0)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS storage_one_universal_scope
ON storage_scopes(scope_type)
WHERE scope_type = 'universal' AND state != 'deleted';

CREATE UNIQUE INDEX IF NOT EXISTS storage_one_workspace_per_chat
ON storage_scopes(owner_chat_id)
WHERE scope_type = 'workspace' AND state != 'deleted';

CREATE TABLE IF NOT EXISTS storage_objects (
    object_id          TEXT PRIMARY KEY,
    plaintext_sha256   BLOB NOT NULL CHECK (length(plaintext_sha256) = 32),
    plaintext_size     INTEGER NOT NULL CHECK (plaintext_size >= 0),
    encrypted_size     INTEGER NOT NULL CHECK (encrypted_size >= 0),
    relative_path      TEXT NOT NULL UNIQUE,
    detected_format    TEXT NOT NULL,
    detected_mime      TEXT,
    encryption_version INTEGER NOT NULL CHECK (encryption_version >= 1),
    integrity_state    TEXT NOT NULL CHECK (
        integrity_state IN ('pending', 'verified', 'corrupt', 'missing', 'quarantined')
    ),
    created_at         TEXT NOT NULL,
    verified_at        TEXT,
    UNIQUE (plaintext_sha256, plaintext_size)
);

CREATE INDEX IF NOT EXISTS storage_objects_integrity_state
ON storage_objects(integrity_state, created_at);

CREATE TABLE IF NOT EXISTS file_versions (
    version_id          TEXT PRIMARY KEY,
    object_id           TEXT NOT NULL,
    previous_version_id TEXT,
    version_number      INTEGER NOT NULL CHECK (version_number >= 1),
    created_by          TEXT NOT NULL CHECK (
        created_by IN (
            'user_import',
            'user_edit',
            'assistant_generation',
            'research',
            'system_recovery'
        )
    ),
    original_filename   TEXT,
    detected_encoding   TEXT,
    language_id         TEXT,
    created_at          TEXT NOT NULL,
    FOREIGN KEY (object_id) REFERENCES storage_objects(object_id) ON DELETE RESTRICT,
    FOREIGN KEY (previous_version_id) REFERENCES file_versions(version_id) ON DELETE RESTRICT,
    UNIQUE (object_id, version_number)
);

CREATE INDEX IF NOT EXISTS file_versions_previous_version
ON file_versions(previous_version_id);

CREATE TABLE IF NOT EXISTS storage_nodes (
    node_id            TEXT PRIMARY KEY,
    scope_id           TEXT NOT NULL,
    parent_node_id     TEXT,
    node_type          TEXT NOT NULL CHECK (node_type IN ('directory', 'file')),
    display_name       TEXT NOT NULL CHECK (length(trim(display_name)) > 0),
    normalized_name    TEXT NOT NULL CHECK (length(normalized_name) > 0),
    current_version_id TEXT,
    system_role        TEXT CHECK (
        system_role IN (
            'scope_root',
            'uploaded_files',
            'generated_files',
            'drafts',
            'research',
            'exports',
            'temporary',
            'trash'
        )
    ),
    state              TEXT NOT NULL CHECK (
        state IN ('active', 'importing', 'trashed', 'quarantined', 'deleting', 'deleted')
    ),
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    trashed_at         TEXT,
    FOREIGN KEY (scope_id) REFERENCES storage_scopes(scope_id) ON DELETE CASCADE,
    FOREIGN KEY (parent_node_id) REFERENCES storage_nodes(node_id) ON DELETE RESTRICT,
    FOREIGN KEY (current_version_id) REFERENCES file_versions(version_id) ON DELETE RESTRICT,
    CHECK (node_type = 'file' OR current_version_id IS NULL),
    CHECK (system_role IS NULL OR node_type = 'directory'),
    CHECK (parent_node_id IS NOT node_id),
    CHECK ((state = 'trashed' AND trashed_at IS NOT NULL) OR state != 'trashed')
);

CREATE UNIQUE INDEX IF NOT EXISTS storage_unique_active_sibling_name
ON storage_nodes(scope_id, COALESCE(parent_node_id, ''), normalized_name)
WHERE state IN ('active', 'importing');

CREATE UNIQUE INDEX IF NOT EXISTS storage_unique_system_role_per_scope
ON storage_nodes(scope_id, system_role)
WHERE system_role IS NOT NULL AND state != 'deleted';

CREATE INDEX IF NOT EXISTS storage_nodes_parent_listing
ON storage_nodes(scope_id, parent_node_id, state, normalized_name);

CREATE INDEX IF NOT EXISTS storage_nodes_current_version
ON storage_nodes(current_version_id)
WHERE current_version_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS import_transactions (
    transaction_id        TEXT PRIMARY KEY,
    target_scope_id       TEXT NOT NULL,
    target_parent_node_id TEXT NOT NULL,
    source_uri_fingerprint TEXT,
    original_filename     TEXT NOT NULL,
    staging_relative_path TEXT NOT NULL UNIQUE,
    expected_size         INTEGER CHECK (expected_size IS NULL OR expected_size >= 0),
    bytes_written         INTEGER NOT NULL DEFAULT 0 CHECK (bytes_written >= 0),
    detected_extension    TEXT,
    detected_mime         TEXT,
    detected_encoding     TEXT,
    state                 TEXT NOT NULL CHECK (
        state IN (
            'created',
            'validating',
            'copying',
            'hashing',
            'encrypting',
            'committing_object',
            'committing_node',
            'indexing',
            'completed',
            'cancel_requested',
            'cancelled',
            'failed',
            'recovering'
        )
    ),
    error_code            TEXT,
    error_details         TEXT,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL,
    completed_at          TEXT,
    FOREIGN KEY (target_scope_id) REFERENCES storage_scopes(scope_id) ON DELETE RESTRICT,
    FOREIGN KEY (target_parent_node_id) REFERENCES storage_nodes(node_id) ON DELETE RESTRICT,
    CHECK (expected_size IS NULL OR bytes_written <= expected_size OR state IN ('failed', 'recovering')),
    CHECK ((state = 'completed' AND completed_at IS NOT NULL) OR state != 'completed')
);

CREATE INDEX IF NOT EXISTS import_transactions_recovery_queue
ON import_transactions(state, updated_at)
WHERE state NOT IN ('completed', 'cancelled', 'failed');

CREATE TABLE IF NOT EXISTS file_indexes (
    index_id            TEXT PRIMARY KEY,
    version_id          TEXT NOT NULL,
    parser_id           TEXT NOT NULL,
    parser_version      INTEGER NOT NULL CHECK (parser_version >= 1),
    chunker_version     INTEGER NOT NULL CHECK (chunker_version >= 1),
    embedding_model_id  TEXT,
    state               TEXT NOT NULL CHECK (
        state IN ('pending', 'parsing', 'chunking', 'embedding', 'ready', 'failed', 'cancelled', 'stale')
    ),
    warning_count       INTEGER NOT NULL DEFAULT 0 CHECK (warning_count >= 0),
    error_code          TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
    UNIQUE (version_id, parser_id, parser_version, chunker_version, embedding_model_id)
);

CREATE INDEX IF NOT EXISTS file_indexes_state
ON file_indexes(state, updated_at);

CREATE TABLE IF NOT EXISTS document_blocks (
    block_id        TEXT PRIMARY KEY,
    index_id        TEXT NOT NULL,
    ordinal         INTEGER NOT NULL CHECK (ordinal >= 0),
    block_type      TEXT NOT NULL,
    start_line      INTEGER CHECK (start_line IS NULL OR start_line >= 1),
    end_line        INTEGER CHECK (end_line IS NULL OR end_line >= 1),
    structured_path TEXT,
    language_id     TEXT,
    text_content    TEXT NOT NULL,
    token_count     INTEGER CHECK (token_count IS NULL OR token_count >= 0),
    metadata_json   TEXT,
    FOREIGN KEY (index_id) REFERENCES file_indexes(index_id) ON DELETE CASCADE,
    UNIQUE (index_id, ordinal),
    CHECK (start_line IS NULL OR end_line IS NULL OR end_line >= start_line),
    CHECK (metadata_json IS NULL OR json_valid(metadata_json))
);

CREATE INDEX IF NOT EXISTS document_blocks_line_lookup
ON document_blocks(index_id, start_line, end_line);

CREATE TABLE IF NOT EXISTS operation_journal (
    journal_id      TEXT PRIMARY KEY,
    operation_type  TEXT NOT NULL,
    scope_id        TEXT,
    node_id         TEXT,
    transaction_id  TEXT,
    phase           TEXT NOT NULL,
    payload_json    TEXT NOT NULL CHECK (json_valid(payload_json)),
    state           TEXT NOT NULL CHECK (
        state IN (
            'prepared',
            'applied_filesystem',
            'applied_database',
            'committed',
            'rolling_back',
            'rolled_back',
            'recovery_required'
        )
    ),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    FOREIGN KEY (scope_id) REFERENCES storage_scopes(scope_id) ON DELETE SET NULL,
    FOREIGN KEY (node_id) REFERENCES storage_nodes(node_id) ON DELETE SET NULL,
    FOREIGN KEY (transaction_id) REFERENCES import_transactions(transaction_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS operation_journal_recovery_queue
ON operation_journal(state, updated_at)
WHERE state NOT IN ('committed', 'rolled_back');

PRAGMA user_version = 14;
