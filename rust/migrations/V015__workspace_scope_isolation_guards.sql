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

-- Scope membership is an identity property. Cross-scope moves must be modeled
-- as explicit copy/reference operations, never as in-place scope reassignment.
CREATE TRIGGER IF NOT EXISTS storage_node_scope_immutable
BEFORE UPDATE OF scope_id ON storage_nodes
WHEN NEW.scope_id IS NOT OLD.scope_id
BEGIN
    SELECT RAISE(ABORT, 'storage node scope membership is immutable');
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

-- Once an import journal row exists, its authorization target is immutable.
-- Recovery may advance state, but it may not silently retarget a published
-- filesystem object to another directory or scope.
CREATE TRIGGER IF NOT EXISTS import_target_identity_immutable
BEFORE UPDATE OF target_scope_id, target_parent_node_id ON import_transactions
WHEN NEW.target_scope_id IS NOT OLD.target_scope_id
  OR NEW.target_parent_node_id IS NOT OLD.target_parent_node_id
BEGIN
    SELECT RAISE(ABORT, 'import target identity is immutable');
END;

-- A scope's ownership and root identity are immutable after creation. Mutable
-- lifecycle fields such as display_name/state remain updateable.
CREATE TRIGGER IF NOT EXISTS storage_scope_identity_immutable
BEFORE UPDATE OF scope_type, workspace_id, owner_chat_id, root_node_id ON storage_scopes
WHEN NEW.scope_type IS NOT OLD.scope_type
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
-- the exact root declared by its owning scope. Scope creation inserts the scope
-- row first and the root node second, so this is enforceable without deferral.
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

-- Journal evidence must never associate a node from one scope with another
-- scope. This closes the audit/recovery boundary, not just the live hierarchy.
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
