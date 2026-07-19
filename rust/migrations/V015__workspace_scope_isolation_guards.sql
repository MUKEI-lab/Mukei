-- Enforce that hierarchical and journal references never cross storage scopes.
--
-- SQLite cannot express these same-scope relationships with the existing
-- single-column foreign keys without rebuilding V014 tables. Triggers keep the
-- append-only migration history intact and fail closed on insert and update.

CREATE TRIGGER IF NOT EXISTS storage_nodes_parent_same_scope_insert
BEFORE INSERT ON storage_nodes
WHEN NEW.parent_node_id IS NOT NULL
BEGIN
    SELECT CASE
        WHEN NOT EXISTS (
            SELECT 1
            FROM storage_nodes AS parent
            WHERE parent.node_id = NEW.parent_node_id
              AND parent.scope_id = NEW.scope_id
              AND parent.node_type = 'directory'
              AND parent.state NOT IN ('deleted', 'deleting')
        )
        THEN RAISE(ABORT, 'storage parent must be an active directory in the same scope')
    END;
END;

CREATE TRIGGER IF NOT EXISTS storage_nodes_parent_same_scope_update
BEFORE UPDATE OF scope_id, parent_node_id ON storage_nodes
WHEN NEW.parent_node_id IS NOT NULL
BEGIN
    SELECT CASE
        WHEN NOT EXISTS (
            SELECT 1
            FROM storage_nodes AS parent
            WHERE parent.node_id = NEW.parent_node_id
              AND parent.scope_id = NEW.scope_id
              AND parent.node_type = 'directory'
              AND parent.state NOT IN ('deleted', 'deleting')
        )
        THEN RAISE(ABORT, 'storage parent must be an active directory in the same scope')
    END;
END;

-- Node identity and scope membership are immutable. Cross-scope moves must be
-- modeled as explicit copy/reference operations rather than in-place rebinding.
CREATE TRIGGER IF NOT EXISTS storage_node_identity_immutable
BEFORE UPDATE OF node_id, scope_id ON storage_nodes
WHEN NEW.node_id IS NOT OLD.node_id
  OR NEW.scope_id IS NOT OLD.scope_id
BEGIN
    SELECT RAISE(ABORT, 'storage node identity and scope membership are immutable');
END;

CREATE TRIGGER IF NOT EXISTS import_target_same_scope_insert
BEFORE INSERT ON import_transactions
BEGIN
    SELECT CASE
        WHEN NOT EXISTS (
            SELECT 1
            FROM storage_nodes AS parent
            WHERE parent.node_id = NEW.target_parent_node_id
              AND parent.scope_id = NEW.target_scope_id
              AND parent.node_type = 'directory'
              AND parent.state = 'active'
        )
        THEN RAISE(ABORT, 'import target must be an active directory in the target scope')
    END;
END;

CREATE TRIGGER IF NOT EXISTS import_target_same_scope_update
BEFORE UPDATE OF target_scope_id, target_parent_node_id ON import_transactions
BEGIN
    SELECT CASE
        WHEN NOT EXISTS (
            SELECT 1
            FROM storage_nodes AS parent
            WHERE parent.node_id = NEW.target_parent_node_id
              AND parent.scope_id = NEW.target_scope_id
              AND parent.node_type = 'directory'
              AND parent.state = 'active'
        )
        THEN RAISE(ABORT, 'import target must be an active directory in the target scope')
    END;
END;

-- Once an import journal row exists, its identity and authorization target are
-- immutable. Recovery may advance state, but may never retarget published work.
CREATE TRIGGER IF NOT EXISTS import_identity_immutable
BEFORE UPDATE OF transaction_id, target_scope_id, target_parent_node_id ON import_transactions
WHEN NEW.transaction_id IS NOT OLD.transaction_id
  OR NEW.target_scope_id IS NOT OLD.target_scope_id
  OR NEW.target_parent_node_id IS NOT OLD.target_parent_node_id
BEGIN
    SELECT RAISE(ABORT, 'import transaction identity and target are immutable');
END;

-- Progress is durable evidence. A retry may repeat the same byte count, but it
-- must never move backwards and make a partially-copied file look less complete.
CREATE TRIGGER IF NOT EXISTS import_progress_monotonic
BEFORE UPDATE OF bytes_written ON import_transactions
WHEN NEW.bytes_written < OLD.bytes_written
BEGIN
    SELECT RAISE(ABORT, 'import progress must be monotonic');
END;

-- Terminal imports are stable facts. Recovery and stale workers may not revive
-- a completed, cancelled, or failed transaction by issuing a direct SQL update.
CREATE TRIGGER IF NOT EXISTS import_terminal_state_immutable
BEFORE UPDATE OF state ON import_transactions
WHEN OLD.state IN ('completed', 'cancelled', 'failed')
  AND NEW.state IS NOT OLD.state
BEGIN
    SELECT RAISE(ABORT, 'terminal import state is immutable');
END;

CREATE TRIGGER IF NOT EXISTS completed_import_timestamp_immutable
BEFORE UPDATE OF completed_at ON import_transactions
WHEN OLD.state = 'completed'
  AND NEW.completed_at IS NOT OLD.completed_at
BEGIN
    SELECT RAISE(ABORT, 'completed import timestamp is immutable');
END;

-- A scope's primary identity, ownership, and root binding are immutable after
-- creation. Mutable lifecycle fields such as display_name/state remain updateable.
CREATE TRIGGER IF NOT EXISTS storage_scope_identity_immutable
BEFORE UPDATE OF scope_id, scope_type, workspace_id, owner_chat_id, root_node_id ON storage_scopes
WHEN NEW.scope_id IS NOT OLD.scope_id
  OR NEW.scope_type IS NOT OLD.scope_type
  OR NEW.workspace_id IS NOT OLD.workspace_id
  OR NEW.owner_chat_id IS NOT OLD.owner_chat_id
  OR NEW.root_node_id IS NOT OLD.root_node_id
BEGIN
    SELECT RAISE(ABORT, 'storage scope identity is immutable');
END;

CREATE TRIGGER IF NOT EXISTS storage_scope_root_identity_insert
BEFORE INSERT ON storage_scopes
BEGIN
    SELECT CASE
        WHEN EXISTS (
            SELECT 1 FROM storage_scopes AS existing
            WHERE existing.root_node_id = NEW.root_node_id
        )
        THEN RAISE(ABORT, 'storage scope root node identity must be unique')
    END;
END;

CREATE UNIQUE INDEX IF NOT EXISTS storage_unique_root_node_id
ON storage_scopes(root_node_id);

-- A node may claim the reserved scope_root role only when its node identity is
-- the exact root declared by its owning scope.
CREATE TRIGGER IF NOT EXISTS storage_scope_root_binding_insert
BEFORE INSERT ON storage_nodes
WHEN NEW.system_role = 'scope_root'
BEGIN
    SELECT CASE
        WHEN NEW.parent_node_id IS NOT NULL
          OR NEW.node_type != 'directory'
          OR NOT EXISTS (
              SELECT 1
              FROM storage_scopes AS scope
              WHERE scope.scope_id = NEW.scope_id
                AND scope.root_node_id = NEW.node_id
          )
        THEN RAISE(ABORT, 'scope root node must match its storage scope root identity')
    END;
END;

CREATE TRIGGER IF NOT EXISTS storage_scope_root_binding_update
BEFORE UPDATE OF node_id, scope_id, parent_node_id, node_type, system_role ON storage_nodes
WHEN OLD.system_role = 'scope_root'
  AND (
      NEW.node_id IS NOT OLD.node_id
      OR NEW.scope_id IS NOT OLD.scope_id
      OR NEW.parent_node_id IS NOT OLD.parent_node_id
      OR NEW.node_type IS NOT OLD.node_type
      OR NEW.system_role IS NOT OLD.system_role
  )
BEGIN
    SELECT RAISE(ABORT, 'scope root identity and binding are immutable');
END;

-- Reserved system-directory roles are structural identities, not mutable labels.
CREATE TRIGGER IF NOT EXISTS storage_system_role_immutable
BEFORE UPDATE OF system_role ON storage_nodes
WHEN OLD.system_role IS NOT NULL
  AND NEW.system_role IS NOT OLD.system_role
BEGIN
    SELECT RAISE(ABORT, 'system directory role is immutable');
END;

-- Immutable object bytes require immutable identity metadata. Integrity state and
-- verification timestamps remain mutable so corruption/quarantine can be recorded.
CREATE TRIGGER IF NOT EXISTS storage_object_identity_immutable
BEFORE UPDATE OF object_id, plaintext_sha256, plaintext_size, encrypted_size, relative_path, encryption_version
ON storage_objects
WHEN NEW.object_id IS NOT OLD.object_id
  OR NEW.plaintext_sha256 IS NOT OLD.plaintext_sha256
  OR NEW.plaintext_size IS NOT OLD.plaintext_size
  OR NEW.encrypted_size IS NOT OLD.encrypted_size
  OR NEW.relative_path IS NOT OLD.relative_path
  OR NEW.encryption_version IS NOT OLD.encryption_version
BEGIN
    SELECT RAISE(ABORT, 'immutable storage object identity metadata cannot change');
END;

-- Version lineage is append-only. A version may be referenced by new logical
-- nodes, but its object and ancestry cannot be rewritten after publication.
CREATE TRIGGER IF NOT EXISTS file_version_identity_immutable
BEFORE UPDATE OF version_id, object_id, previous_version_id, version_number ON file_versions
WHEN NEW.version_id IS NOT OLD.version_id
  OR NEW.object_id IS NOT OLD.object_id
  OR NEW.previous_version_id IS NOT OLD.previous_version_id
  OR NEW.version_number IS NOT OLD.version_number
BEGIN
    SELECT RAISE(ABORT, 'file version identity and lineage are immutable');
END;

-- Journal evidence must never associate a node from one scope with another.
CREATE TRIGGER IF NOT EXISTS operation_journal_node_same_scope_insert
BEFORE INSERT ON operation_journal
WHEN NEW.node_id IS NOT NULL
BEGIN
    SELECT CASE
        WHEN NEW.scope_id IS NULL
          OR NOT EXISTS (
              SELECT 1
              FROM storage_nodes AS node
              WHERE node.node_id = NEW.node_id
                AND node.scope_id = NEW.scope_id
          )
        THEN RAISE(ABORT, 'operation journal node must belong to the journal scope')
    END;
END;

CREATE TRIGGER IF NOT EXISTS operation_journal_node_same_scope_update
BEFORE UPDATE OF scope_id, node_id ON operation_journal
WHEN NEW.node_id IS NOT NULL
BEGIN
    SELECT CASE
        WHEN NEW.scope_id IS NULL
          OR NOT EXISTS (
              SELECT 1
              FROM storage_nodes AS node
              WHERE node.node_id = NEW.node_id
                AND node.scope_id = NEW.scope_id
          )
        THEN RAISE(ABORT, 'operation journal node must belong to the journal scope')
    END;
END;

-- Import-backed journal entries must retain the same target scope as their
-- import transaction so recovery cannot replay evidence under another scope.
CREATE TRIGGER IF NOT EXISTS operation_journal_transaction_same_scope_insert
BEFORE INSERT ON operation_journal
WHEN NEW.transaction_id IS NOT NULL
BEGIN
    SELECT CASE
        WHEN NEW.scope_id IS NULL
          OR NOT EXISTS (
              SELECT 1
              FROM import_transactions AS import_tx
              WHERE import_tx.transaction_id = NEW.transaction_id
                AND import_tx.target_scope_id = NEW.scope_id
          )
        THEN RAISE(ABORT, 'operation journal transaction must belong to the journal scope')
    END;
END;

CREATE TRIGGER IF NOT EXISTS operation_journal_transaction_same_scope_update
BEFORE UPDATE OF scope_id, transaction_id ON operation_journal
WHEN NEW.transaction_id IS NOT NULL
BEGIN
    SELECT CASE
        WHEN NEW.scope_id IS NULL
          OR NOT EXISTS (
              SELECT 1
              FROM import_transactions AS import_tx
              WHERE import_tx.transaction_id = NEW.transaction_id
                AND import_tx.target_scope_id = NEW.scope_id
          )
        THEN RAISE(ABORT, 'operation journal transaction must belong to the journal scope')
    END;
END;

-- Journal primary identity is immutable from creation. Once committed or rolled
-- back, its evidence payload and associations are frozen as an audit fact.
CREATE TRIGGER IF NOT EXISTS operation_journal_identity_immutable
BEFORE UPDATE OF journal_id ON operation_journal
WHEN NEW.journal_id IS NOT OLD.journal_id
BEGIN
    SELECT RAISE(ABORT, 'operation journal identity is immutable');
END;

CREATE TRIGGER IF NOT EXISTS operation_journal_terminal_evidence_immutable
BEFORE UPDATE OF operation_type, scope_id, node_id, transaction_id, phase, payload_json, state ON operation_journal
WHEN OLD.state IN ('committed', 'rolled_back')
  AND (
      NEW.operation_type IS NOT OLD.operation_type
      OR NEW.scope_id IS NOT OLD.scope_id
      OR NEW.node_id IS NOT OLD.node_id
      OR NEW.transaction_id IS NOT OLD.transaction_id
      OR NEW.phase IS NOT OLD.phase
      OR NEW.payload_json IS NOT OLD.payload_json
      OR NEW.state IS NOT OLD.state
  )
BEGIN
    SELECT RAISE(ABORT, 'terminal operation journal evidence is immutable');
END;
