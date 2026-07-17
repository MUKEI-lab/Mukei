#![cfg(feature = "rusqlite")]

use rusqlite::{params, Connection};

const V014: &str = include_str!("../../../migrations/V014__universal_storage_and_workspaces.sql");

fn migrated_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("open in-memory SQLite");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign keys");
    connection
        .execute_batch(V014)
        .expect("V014 must apply to an empty database");
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
                format!("root-{suffix}"),
            ],
        )
        .expect("insert workspace scope");

    connection
        .execute(
            "INSERT INTO storage_nodes (
                node_id, scope_id, parent_node_id, node_type, display_name,
                normalized_name, current_version_id, system_role, state,
                created_at, updated_at, trashed_at
             ) VALUES (?1, ?2, NULL, 'directory', 'Workspace', 'workspace',
                       NULL, 'scope_root', 'active', CURRENT_TIMESTAMP,
                       CURRENT_TIMESTAMP, NULL)",
            params![format!("root-{suffix}"), format!("scope-{suffix}")],
        )
        .expect("insert workspace root");

    connection
        .execute(
            "INSERT INTO storage_nodes (
                node_id, scope_id, parent_node_id, node_type, display_name,
                normalized_name, current_version_id, system_role, state,
                created_at, updated_at, trashed_at
             ) VALUES (?1, ?2, ?3, 'directory', 'Uploaded files', 'uploaded files',
                       NULL, 'uploaded_files', 'active', CURRENT_TIMESTAMP,
                       CURRENT_TIMESTAMP, NULL)",
            params![
                format!("uploads-{suffix}"),
                format!("scope-{suffix}"),
                format!("root-{suffix}"),
            ],
        )
        .expect("insert Uploaded files directory");
}

#[test]
fn node_parent_cannot_cross_workspace_scope_boundary() {
    let connection = migrated_connection();
    insert_workspace(&connection, "a");
    insert_workspace(&connection, "b");

    let result = connection.execute(
        "INSERT INTO storage_nodes (
            node_id, scope_id, parent_node_id, node_type, display_name,
            normalized_name, current_version_id, system_role, state,
            created_at, updated_at, trashed_at
         ) VALUES ('cross-chat-file', 'scope-a', 'uploads-b', 'file', 'notes.txt',
                   'notes.txt', NULL, NULL, 'active', CURRENT_TIMESTAMP,
                   CURRENT_TIMESTAMP, NULL)",
        [],
    );

    assert!(
        result.is_err(),
        "a node must never be attached to a parent owned by another chat workspace"
    );
}

#[test]
fn import_target_cannot_cross_workspace_scope_boundary() {
    let connection = migrated_connection();
    insert_workspace(&connection, "a");
    insert_workspace(&connection, "b");

    let result = connection.execute(
        "INSERT INTO import_transactions (
            transaction_id, target_scope_id, target_parent_node_id,
            original_filename, staging_relative_path, bytes_written, state,
            created_at, updated_at
         ) VALUES ('import-cross-chat', 'scope-a', 'uploads-b', 'notes.txt',
                   'staging/import-cross-chat.partial', 0, 'validating',
                   CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        [],
    );

    assert!(
        result.is_err(),
        "an import journal must not target a directory from another workspace"
    );
}

#[test]
fn workspace_lookup_is_bound_to_both_workspace_and_chat_identity() {
    let connection = migrated_connection();
    insert_workspace(&connection, "a");
    insert_workspace(&connection, "b");

    let visible_to_chat_a: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM storage_scopes
             WHERE workspace_id = 'workspace-a'
               AND owner_chat_id = 'chat-a'
               AND state = 'active'",
            [],
            |row| row.get(0),
        )
        .expect("authorized workspace lookup");
    assert_eq!(visible_to_chat_a, 1);

    let visible_with_wrong_chat: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM storage_scopes
             WHERE workspace_id = 'workspace-a'
               AND owner_chat_id = 'chat-b'
               AND state = 'active'",
            [],
            |row| row.get(0),
        )
        .expect("unauthorized workspace lookup");
    assert_eq!(visible_with_wrong_chat, 0);
}
