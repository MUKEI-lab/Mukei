#![cfg(feature = "rusqlite")]

use rusqlite::{params, Connection};

const V014: &str = include_str!("../../../migrations/V014__universal_storage_and_workspaces.sql");
const V015: &str = include_str!("../../../migrations/V015__workspace_scope_isolation_guards.sql");

fn migrated_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("open in-memory SQLite");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .unwrap();
    connection.execute_batch(V014).unwrap();
    connection.execute_batch(V015).unwrap();
    connection
}

fn insert_workspace(connection: &Connection, suffix: &str) {
    connection
        .execute(
            "INSERT INTO storage_scopes (
            scope_id, workspace_id, scope_type, owner_chat_id, root_node_id,
            display_name, state, created_at, updated_at
         ) VALUES (?1, ?2, 'workspace', ?3, ?4, 'Workspace', 'active',
                   CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            params![
                format!("scope-{suffix}"),
                format!("workspace-{suffix}"),
                format!("chat-{suffix}"),
                format!("root-{suffix}")
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO storage_nodes (
            node_id, scope_id, parent_node_id, node_type, display_name,
            normalized_name, system_role, state, created_at, updated_at
         ) VALUES (?1, ?2, NULL, 'directory', 'Workspace', 'workspace',
                   'scope_root', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            params![format!("root-{suffix}"), format!("scope-{suffix}")],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO storage_nodes (
            node_id, scope_id, parent_node_id, node_type, display_name,
            normalized_name, system_role, state, created_at, updated_at
         ) VALUES (?1, ?2, ?3, 'directory', 'Uploaded files', 'uploaded files',
                   'uploaded_files', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            params![
                format!("uploads-{suffix}"),
                format!("scope-{suffix}"),
                format!("root-{suffix}")
            ],
        )
        .unwrap();
}

#[test]
fn node_parent_must_belong_to_same_scope() {
    let connection = migrated_connection();
    insert_workspace(&connection, "a");
    insert_workspace(&connection, "b");

    let result = connection.execute(
        "INSERT INTO storage_nodes (
            node_id, scope_id, parent_node_id, node_type, display_name,
            normalized_name, state, created_at, updated_at
         ) VALUES ('cross-chat', 'scope-a', 'uploads-b', 'file', 'notes.txt',
                   'notes.txt', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        [],
    );
    assert!(result.is_err());
}

#[test]
fn import_target_must_belong_to_same_scope() {
    let connection = migrated_connection();
    insert_workspace(&connection, "a");
    insert_workspace(&connection, "b");

    let result = connection.execute(
        "INSERT INTO import_transactions (
            transaction_id, target_scope_id, target_parent_node_id,
            original_filename, staging_relative_path, state, created_at, updated_at
         ) VALUES ('import-cross-chat', 'scope-a', 'uploads-b', 'notes.txt',
                   'staging/import-cross-chat.partial', 'created',
                   CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        [],
    );
    assert!(result.is_err());
}

#[test]
fn same_scope_parent_and_import_target_are_accepted() {
    let connection = migrated_connection();
    insert_workspace(&connection, "a");

    connection
        .execute(
            "INSERT INTO storage_nodes (
            node_id, scope_id, parent_node_id, node_type, display_name,
            normalized_name, state, created_at, updated_at
         ) VALUES ('file-a', 'scope-a', 'uploads-a', 'file', 'notes.txt',
                   'notes.txt', 'importing', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();

    connection
        .execute(
            "INSERT INTO import_transactions (
            transaction_id, target_scope_id, target_parent_node_id,
            original_filename, staging_relative_path, state, created_at, updated_at
         ) VALUES ('import-a', 'scope-a', 'uploads-a', 'notes.txt',
                   'staging/import-a.partial', 'created',
                   CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
}
