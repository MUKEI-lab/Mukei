#![cfg(feature = "rusqlite")]

//! Adversarial regression coverage for the forward-only V016 storage hardening.

use rusqlite::{params, Connection};

const V014: &str = include_str!("../../../migrations/V014__universal_storage_and_workspaces.sql");
const V015: &str = include_str!("../../../migrations/V015__workspace_scope_isolation_guards.sql");
const V016: &str =
    include_str!("../../../migrations/V016__storage_identity_and_recovery_hardening.sql");

fn migrated_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("open in-memory SQLite");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign keys");
    connection.execute_batch(V014).expect("apply V014");
    connection.execute_batch(V015).expect("apply V015");
    connection.execute_batch(V016).expect("apply V016");
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
fn scope_node_and_system_directory_identity_fail_closed() {
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
        .unwrap();

    assert!(connection
        .execute(
            "UPDATE storage_nodes SET scope_id = 'scope-b', parent_node_id = 'root-b'
             WHERE node_id = 'scratch-a'",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE storage_nodes SET node_id = 'scratch-renamed' WHERE node_id = 'scratch-a'",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE storage_scopes SET owner_chat_id = 'chat-other' WHERE scope_id = 'scope-a'",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE storage_scopes SET scope_id = 'scope-renamed' WHERE scope_id = 'scope-a'",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE storage_nodes SET system_role = NULL WHERE node_id = 'uploads-a'",
            [],
        )
        .is_err());

    connection
        .execute(
            "UPDATE storage_scopes SET display_name = 'Renamed Workspace' WHERE scope_id = 'scope-a'",
            [],
        )
        .expect("non-identity metadata remains mutable");
}

#[test]
fn declared_scope_root_cannot_be_spoofed_or_rebound() {
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
        .unwrap();

    assert!(connection
        .execute(
            "INSERT INTO storage_nodes (
                node_id, scope_id, parent_node_id, node_type, display_name,
                normalized_name, system_role, state, created_at, updated_at
             ) VALUES ('rogue-root', 'scope-c', NULL, 'directory', 'Workspace',
                       'workspace', 'scope_root', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .is_err());

    connection
        .execute(
            "INSERT INTO storage_nodes (
                node_id, scope_id, parent_node_id, node_type, display_name,
                normalized_name, system_role, state, created_at, updated_at
             ) VALUES ('root-c', 'scope-c', NULL, 'directory', 'Workspace',
                       'workspace', 'scope_root', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();

    assert!(connection
        .execute(
            "UPDATE storage_nodes SET parent_node_id = 'root-c' WHERE node_id = 'root-c'",
            [],
        )
        .is_err());
}

#[test]
fn import_identity_progress_and_terminal_state_are_stable() {
    let connection = migrated_connection();
    insert_workspace(&connection, "a");
    insert_workspace(&connection, "b");
    insert_import(&connection, "a");

    assert!(connection
        .execute(
            "UPDATE import_transactions SET target_parent_node_id = 'root-a'
             WHERE transaction_id = 'import-a'",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE import_transactions
             SET target_scope_id = 'scope-b', target_parent_node_id = 'uploads-b'
             WHERE transaction_id = 'import-a'",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE import_transactions SET transaction_id = 'import-renamed'
             WHERE transaction_id = 'import-a'",
            [],
        )
        .is_err());

    connection
        .execute(
            "UPDATE import_transactions SET bytes_written = 128 WHERE transaction_id = 'import-a'",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE import_transactions SET bytes_written = 128 WHERE transaction_id = 'import-a'",
            [],
        )
        .expect("idempotent progress replay is allowed");
    assert!(connection
        .execute(
            "UPDATE import_transactions SET bytes_written = 64 WHERE transaction_id = 'import-a'",
            [],
        )
        .is_err());

    connection
        .execute(
            "UPDATE import_transactions SET state = 'failed' WHERE transaction_id = 'import-a'",
            [],
        )
        .unwrap();
    assert!(connection
        .execute(
            "UPDATE import_transactions SET state = 'recovering' WHERE transaction_id = 'import-a'",
            [],
        )
        .is_err());
}

#[test]
fn immutable_object_and_version_metadata_cannot_be_rewritten() {
    let connection = migrated_connection();
    connection
        .execute(
            "INSERT INTO storage_objects (
                object_id, plaintext_sha256, plaintext_size, encrypted_size,
                relative_path, detected_format, encryption_version, integrity_state,
                created_at, verified_at
             ) VALUES ('object-a', zeroblob(32), 12, 64, 'aa/bb/object-a.mobj',
                       'text', 1, 'verified', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO file_versions (
                version_id, object_id, previous_version_id, version_number, created_by,
                original_filename, created_at
             ) VALUES ('version-a', 'object-a', NULL, 1, 'user_import', 'notes.txt',
                       CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();

    assert!(connection
        .execute(
            "UPDATE storage_objects SET relative_path = 'cc/dd/rewritten.mobj'
             WHERE object_id = 'object-a'",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE storage_objects SET plaintext_size = 13 WHERE object_id = 'object-a'",
            [],
        )
        .is_err());
    connection
        .execute(
            "UPDATE storage_objects SET integrity_state = 'quarantined' WHERE object_id = 'object-a'",
            [],
        )
        .expect("integrity lifecycle remains mutable");
    assert!(connection
        .execute(
            "UPDATE file_versions SET version_number = 2 WHERE version_id = 'version-a'",
            [],
        )
        .is_err());
}

#[test]
fn operation_journal_evidence_is_scope_bound_and_terminally_frozen() {
    let connection = migrated_connection();
    insert_workspace(&connection, "a");
    insert_workspace(&connection, "b");
    insert_import(&connection, "a");

    assert!(connection
        .execute(
            "INSERT INTO operation_journal (
                journal_id, operation_type, scope_id, node_id, transaction_id,
                phase, payload_json, state, created_at, updated_at
             ) VALUES ('journal-cross-tx', 'storage_import_commit', 'scope-b', NULL,
                       'import-a', 'prepared', '{}', 'prepared',
                       CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO operation_journal (
                journal_id, operation_type, scope_id, node_id, transaction_id,
                phase, payload_json, state, created_at, updated_at
             ) VALUES ('journal-cross-node', 'storage_test', 'scope-a', 'uploads-b',
                       NULL, 'prepared', '{}', 'prepared',
                       CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .is_err());

    connection
        .execute(
            "INSERT INTO operation_journal (
                journal_id, operation_type, scope_id, node_id, transaction_id,
                phase, payload_json, state, created_at, updated_at
             ) VALUES ('journal-valid', 'storage_import_commit', 'scope-a', 'uploads-a',
                       'import-a', 'database_committed', '{}', 'committed',
                       CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();

    assert!(connection
        .execute(
            "UPDATE operation_journal SET node_id = 'uploads-b' WHERE journal_id = 'journal-valid'",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE operation_journal SET payload_json = '{\"tampered\":true}'
             WHERE journal_id = 'journal-valid'",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE operation_journal SET state = 'recovery_required'
             WHERE journal_id = 'journal-valid'",
            [],
        )
        .is_err());
}
