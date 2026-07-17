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

#[test]
fn migration_is_idempotent_and_records_policy_metadata() {
    let connection = migrated_connection();
    connection
        .execute_batch(V014)
        .expect("V014 CREATE/INSERT statements must be idempotent");

    let metadata: (i64, i64) = connection
        .query_row(
            "SELECT schema_version, file_policy_version FROM storage_schema_metadata WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("storage metadata row");
    assert_eq!(metadata, (1, 1));
}

#[test]
fn workspace_requires_an_immutable_workspace_and_chat_identity() {
    let connection = migrated_connection();

    let missing_workspace_id = connection.execute(
        "INSERT INTO storage_scopes (
            scope_id, workspace_id, scope_type, owner_chat_id, root_node_id,
            display_name, state, created_at, updated_at
         ) VALUES ('scope-a', NULL, 'workspace', 'chat-a', 'root-a', 'Workspace',
                   'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        [],
    );
    assert!(missing_workspace_id.is_err());

    connection
        .execute(
            "INSERT INTO storage_scopes (
                scope_id, workspace_id, scope_type, owner_chat_id, root_node_id,
                display_name, state, created_at, updated_at
             ) VALUES (?1, ?2, 'workspace', ?3, ?4, 'Workspace',
                       'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            params!["scope-a", "workspace-a", "chat-a", "root-a"],
        )
        .expect("valid workspace scope");

    let duplicate_chat = connection.execute(
        "INSERT INTO storage_scopes (
            scope_id, workspace_id, scope_type, owner_chat_id, root_node_id,
            display_name, state, created_at, updated_at
         ) VALUES (?1, ?2, 'workspace', ?3, ?4, 'Workspace',
                   'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        params!["scope-b", "workspace-b", "chat-a", "root-b"],
    );
    assert!(
        duplicate_chat.is_err(),
        "one chat must own at most one live workspace"
    );
}

#[test]
fn object_rows_reject_invalid_hashes_and_node_names_do_not_silently_collide() {
    let connection = migrated_connection();

    let invalid_hash = connection.execute(
        "INSERT INTO storage_objects (
            object_id, plaintext_sha256, plaintext_size, encrypted_size,
            relative_path, detected_format, encryption_version, integrity_state, created_at
         ) VALUES ('object-a', zeroblob(31), 1, 17, 'objects/a', 'text', 1,
                   'verified', CURRENT_TIMESTAMP)",
        [],
    );
    assert!(
        invalid_hash.is_err(),
        "object identity requires a full SHA-256 digest"
    );

    connection
        .execute(
            "INSERT INTO storage_scopes (
                scope_id, workspace_id, scope_type, owner_chat_id, root_node_id,
                display_name, state, created_at, updated_at
             ) VALUES ('scope-a', 'workspace-a', 'workspace', 'chat-a', 'root-a',
                       'Workspace', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .expect("workspace scope");

    connection
        .execute(
            "INSERT INTO storage_nodes (
                node_id, scope_id, parent_node_id, node_type, display_name,
                normalized_name, current_version_id, system_role, state,
                created_at, updated_at, trashed_at
             ) VALUES ('root-a', 'scope-a', NULL, 'directory', 'Workspace',
                       'workspace', NULL, 'scope_root', 'active',
                       CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL)",
            [],
        )
        .expect("workspace root");

    connection
        .execute(
            "INSERT INTO storage_nodes (
                node_id, scope_id, parent_node_id, node_type, display_name,
                normalized_name, current_version_id, system_role, state,
                created_at, updated_at, trashed_at
             ) VALUES ('upload-a', 'scope-a', 'root-a', 'directory', 'Uploaded files',
                       'uploaded files', NULL, 'uploaded_files', 'active',
                       CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL)",
            [],
        )
        .expect("mandatory Uploaded files directory");

    let duplicate_name = connection.execute(
        "INSERT INTO storage_nodes (
            node_id, scope_id, parent_node_id, node_type, display_name,
            normalized_name, current_version_id, system_role, state,
            created_at, updated_at, trashed_at
         ) VALUES ('upload-b', 'scope-a', 'root-a', 'directory', 'UPLOADED FILES',
                   'uploaded files', NULL, NULL, 'active',
                   CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL)",
        [],
    );
    assert!(
        duplicate_name.is_err(),
        "normalized sibling names must not overwrite or collide"
    );
}

#[test]
fn incomplete_imports_remain_recoverable_after_restart() {
    let connection = migrated_connection();
    connection
        .execute(
            "INSERT INTO storage_scopes (
                scope_id, workspace_id, scope_type, owner_chat_id, root_node_id,
                display_name, state, created_at, updated_at
             ) VALUES ('scope-a', 'workspace-a', 'workspace', 'chat-a', 'root-a',
                       'Workspace', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .expect("workspace scope");
    connection
        .execute(
            "INSERT INTO storage_nodes (
                node_id, scope_id, parent_node_id, node_type, display_name,
                normalized_name, current_version_id, system_role, state,
                created_at, updated_at, trashed_at
             ) VALUES ('root-a', 'scope-a', NULL, 'directory', 'Workspace',
                       'workspace', NULL, 'scope_root', 'active',
                       CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL)",
            [],
        )
        .expect("workspace root");

    connection
        .execute(
            "INSERT INTO import_transactions (
                transaction_id, target_scope_id, target_parent_node_id,
                original_filename, staging_relative_path, bytes_written, state,
                created_at, updated_at
             ) VALUES ('import-a', 'scope-a', 'root-a', 'notes.txt',
                       'staging/import-a.partial', 128, 'encrypting',
                       CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .expect("in-flight import journal row");

    let recoverable: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM import_transactions
             WHERE state NOT IN ('completed', 'cancelled', 'failed')",
            [],
            |row| row.get(0),
        )
        .expect("recovery query");
    assert_eq!(recoverable, 1);
}
