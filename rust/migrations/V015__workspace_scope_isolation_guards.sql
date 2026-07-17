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
