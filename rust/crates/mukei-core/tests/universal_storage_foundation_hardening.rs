#![cfg(feature = "rusqlite")]

//! Adversarial regression coverage for storage identity and recovery isolation.
//!
//! These tests intentionally exercise mutations that remain relationally valid
//! at the foreign-key level but violate the Universal Storage security model.

use rusqlite::{params, Connection};

const V014: &str = include_str!("../../../migrations/V014__universal_storage_and_workspaces.sql");
const V015: &str = include_str!("../../../migrations/V015__workspace_scope_isolation_guards.sql");

fn migrated_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("open in-memory SQLite");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign keys");
    connection.execute_batch(V014).expect("apply V014");
    connection.execute_batch(V015).expect("apply V015");
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
        .expect("insert workspace scope");

    connection
        .execute(
            "INSERT INTO storage_nodes (
                node_id, scope_id, parent_node_id, node_type, display_name,
                normalized_name, system_role, state, created_at, updated_at
             ) VALUES (?1, ?2, NULL, 'directory', 'Workspace', 'workspace',
                       'scope_root', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            params![format!("root-{suffix}"), format!("scope-{suffix}")],
        )
        .expect("insert workspace root");

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
        .expect("insert uploaded files directory");
}

fn insert_import(connection: &Connection, suffix: &str) {
    connection
        .execute(
            "INSERT INTO import_transactions (
                transaction_id, target_scope_id, target_parent_node_id,
                original_filename, staging_relative_path, state, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 'notes.txt', ?4, 'created',
                       CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            params![
                format!("import-{suffix}"),
                format!("scope-{suffix}"),
                format!("uploads-{suffix}"),
                format!("staging/import-{suffix}.partial")
            ],
        )
        .expect("insert import transaction");
}

#[test]
fn scope_identity_and_node_membership_cannot_be_rebound() {
    let connection = migrated_connection();
    insert_workspace(&connection, "a");
    insert_workspace(&connection, "b");

    connection
        .execute(
            "INSERT INTO storage_nodes (
                node_id, scope_id, parent_node_id, node_type, display_name,
                normalized_name, state, created_at, updated_at
             ) VALUES ('scratch-a', 'scope-a', 'root-a', 'directory', 'Scratch',
                       'scratch', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .expect("insert ordinary directory");

    let rebind_scope = connection.execute(
        "UPDATE storage_nodes
         SET scope_id = 'scope-b', parent_node_id = 'root-b'
         WHERE node_id = 'scratch-a'",
        [],
    );
    assert!(
        rebind_scope.is_err(),
        "a relationally valid cross-scope move must still fail closed"
    );

    let mutate_owner = connection.execute(
        "UPDATE storage_scopes SET owner_chat_id = 'chat-a-rebound' WHERE scope_id = 'scope-a'",
        [],
    );
    assert!(
        mutate_owner.is_err(),
        "workspace ownership is an immutable identity property"
    );

    connection
        .execute(
            "UPDATE storage_scopes SET display_name = 'Renamed Workspace' WHERE scope_id = 'scope-a'",
            [],
        )
        .expect("non-identity scope metadata remains mutable");
}

#[test]
fn scope_root_role_must_match_the_declared_root_node() {
    let connection = migrated_connection();

    connection
        .execute(
            "INSERT INTO storage_scopes (
                scope_id, workspace_id, scope_type, owner_chat_id, root_node_id,
                display_name, state, created_at, updated_at
             ) VALUES ('scope-c', 'workspace-c', 'workspace', 'chat-c', 'root-c',
                       'Workspace', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .expect("insert scope before its root node");

    let rogue_root = connection.execute(
        "INSERT INTO storage_nodes (
            node_id, scope_id, parent_node_id, node_type, display_name,
            normalized_name, system_role, state, created_at, updated_at
         ) VALUES ('rogue-root-c', 'scope-c', NULL, 'directory', 'Workspace',
                   'workspace', 'scope_root', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        [],
    );
    assert!(
        rogue_root.is_err(),
        "reserved scope_root role must bind to storage_scopes.root_node_id"
    );

    connection
        .execute(
            "INSERT INTO storage_nodes (
                node_id, scope_id, parent_node_id, node_type, display_name,
                normalized_name, system_role, state, created_at, updated_at
             ) VALUES ('root-c', 'scope-c', NULL, 'directory', 'Workspace',
                       'workspace', 'scope_root', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .expect("declared root identity is accepted");
}

#[test]
fn import_authorization_target_is_immutable_after_creation() {
    let connection = migrated_connection();
    insert_workspace(&connection, "a");
    insert_workspace(&connection, "b");
    insert_import(&connection, "a");

    let same_scope_retarget = connection.execute(
        "UPDATE import_transactions
         SET target_parent_node_id = 'root-a'
         WHERE transaction_id = 'import-a'",
        [],
    );
    assert!(
        same_scope_retarget.is_err(),
        "recovery must not silently retarget an import even within one scope"
    );

    let cross_scope_retarget = connection.execute(
        "UPDATE import_transactions
         SET target_scope_id = 'scope-b', target_parent_node_id = 'uploads-b'
         WHERE transaction_id = 'import-a'",
        [],
    );
    assert!(
        cross_scope_retarget.is_err(),
        "a valid destination in another workspace must not permit authorization rebinding"
    );
}

#[test]
fn operation_journal_evidence_cannot_cross_scope_boundaries() {
    let connection = migrated_connection();
    insert_workspace(&connection, "a");
    insert_workspace(&connection, "b");
    insert_import(&connection, "a");

    let cross_scope_transaction = connection.execute(
        "INSERT INTO operation_journal (
            journal_id, operation_type, scope_id, node_id, transaction_id,
            phase, payload_json, state, created_at, updated_at
         ) VALUES ('journal-cross-tx', 'storage_import_commit', 'scope-b', NULL,
                   'import-a', 'prepared', '{}', 'prepared',
                   CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        [],
    );
    assert!(
        cross_scope_transaction.is_err(),
        "journal transaction evidence must stay in the import target scope"
    );

    let cross_scope_node = connection.execute(
        "INSERT INTO operation_journal (
            journal_id, operation_type, scope_id, node_id, transaction_id,
            phase, payload_json, state, created_at, updated_at
         ) VALUES ('journal-cross-node', 'storage_test', 'scope-a', 'uploads-b',
                   NULL, 'prepared', '{}', 'prepared',
                   CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        [],
    );
    assert!(
        cross_scope_node.is_err(),
        "journal node evidence must belong to the declared journal scope"
    );

    connection
        .execute(
            "INSERT INTO operation_journal (
                journal_id, operation_type, scope_id, node_id, transaction_id,
                phase, payload_json, state, created_at, updated_at
             ) VALUES ('journal-valid', 'storage_import_commit', 'scope-a', 'uploads-a',
                       'import-a', 'prepared', '{}', 'prepared',
                       CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .expect("same-scope journal evidence is accepted");

    let cross_scope_update = connection.execute(
        "UPDATE operation_journal SET node_id = 'uploads-b' WHERE journal_id = 'journal-valid'",
        [],
    );
    assert!(
        cross_scope_update.is_err(),
        "journal evidence cannot be rebound to a node in another scope"
    );
}
