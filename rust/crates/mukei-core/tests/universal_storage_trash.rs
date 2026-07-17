#![cfg(feature = "rusqlite")]

use rusqlite::{params, Connection};

const V014: &str = include_str!("../../../migrations/V014__universal_storage_and_workspaces.sql");

fn workspace_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("open in-memory SQLite");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign keys");
    connection.execute_batch(V014).expect("apply V014");

    connection
        .execute(
            "INSERT INTO storage_scopes (
                scope_id, workspace_id, scope_type, owner_chat_id, root_node_id,
                display_name, state, created_at, updated_at
             ) VALUES ('scope-a', 'workspace-a', 'workspace', 'chat-a', 'root-a',
                       'Workspace', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .expect("insert workspace scope");

    for (node_id, parent_node_id, name, normalized_name, role) in [
        ("root-a", None, "Workspace", "workspace", Some("scope_root")),
        (
            "uploaded-a",
            Some("root-a"),
            "Uploaded files",
            "uploaded files",
            Some("uploaded_files"),
        ),
        ("trash-a", Some("root-a"), "Trash", "trash", Some("trash")),
        ("file-a", Some("uploaded-a"), "notes.txt", "notes.txt", None),
    ] {
        connection
            .execute(
                "INSERT INTO storage_nodes (
                    node_id, scope_id, parent_node_id, node_type, display_name,
                    normalized_name, current_version_id, system_role, state,
                    created_at, updated_at, trashed_at
                 ) VALUES (?1, 'scope-a', ?2, ?3, ?4, ?5, NULL, ?6, 'active',
                           CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL)",
                params![
                    node_id,
                    parent_node_id,
                    if role.is_some() { "directory" } else { "file" },
                    name,
                    normalized_name,
                    role,
                ],
            )
            .expect("insert workspace node");
    }

    connection
}

#[test]
fn trash_move_preserves_identity_and_records_original_parent() {
    let connection = workspace_connection();
    let transaction = connection
        .unchecked_transaction()
        .expect("begin transaction");
    transaction
        .execute(
            "INSERT INTO operation_journal (
                journal_id, operation_type, scope_id, node_id, transaction_id,
                phase, payload_json, state, created_at, updated_at
             ) VALUES (
                'journal-trash', 'trash_node', 'scope-a', 'file-a', NULL,
                'database_move',
                '{\"original_parent_node_id\":\"uploaded-a\",\"trash_node_id\":\"trash-a\"}',
                'prepared', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
             )",
            [],
        )
        .expect("prepare trash journal");
    transaction
        .execute(
            "UPDATE storage_nodes
             SET parent_node_id = 'trash-a', state = 'trashed',
                 trashed_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
             WHERE node_id = 'file-a' AND state = 'active'",
            [],
        )
        .expect("move node into Trash");
    transaction
        .execute(
            "UPDATE operation_journal
             SET phase = 'complete', state = 'committed', updated_at = CURRENT_TIMESTAMP
             WHERE journal_id = 'journal-trash'",
            [],
        )
        .expect("commit journal");
    transaction.commit().expect("commit trash move");

    let node: (String, String, i64) = connection
        .query_row(
            "SELECT parent_node_id, state, trashed_at IS NOT NULL
             FROM storage_nodes WHERE node_id = 'file-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("trashed node");
    assert_eq!(node, ("trash-a".to_string(), "trashed".to_string(), 1));

    let original_parent: String = connection
        .query_row(
            "SELECT json_extract(payload_json, '$.original_parent_node_id')
             FROM operation_journal WHERE journal_id = 'journal-trash'",
            [],
            |row| row.get(0),
        )
        .expect("journaled parent");
    assert_eq!(original_parent, "uploaded-a");
}

#[test]
fn system_directories_are_not_valid_trash_candidates() {
    let connection = workspace_connection();
    let candidate_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM storage_nodes
             WHERE node_id = 'uploaded-a' AND state = 'active' AND system_role IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("count candidates");
    assert_eq!(
        candidate_count, 0,
        "mandatory directories must never match trash updates"
    );
}

#[test]
fn restore_conflict_fails_without_overwriting_existing_node() {
    let connection = workspace_connection();
    connection
        .execute(
            "UPDATE storage_nodes
             SET parent_node_id = 'trash-a', state = 'trashed', trashed_at = CURRENT_TIMESTAMP
             WHERE node_id = 'file-a'",
            [],
        )
        .expect("trash original node");
    connection
        .execute(
            "INSERT INTO storage_nodes (
                node_id, scope_id, parent_node_id, node_type, display_name,
                normalized_name, current_version_id, system_role, state,
                created_at, updated_at, trashed_at
             ) VALUES (
                'file-conflict', 'scope-a', 'uploaded-a', 'file', 'notes.txt',
                'notes.txt', NULL, NULL, 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL
             )",
            [],
        )
        .expect("insert conflicting live node");

    let restore = connection.execute(
        "UPDATE storage_nodes
         SET parent_node_id = 'uploaded-a', state = 'active', trashed_at = NULL
         WHERE node_id = 'file-a' AND state = 'trashed'",
        [],
    );
    assert!(
        restore.is_err(),
        "unique sibling-name protection must reject restore conflict"
    );

    let state: String = connection
        .query_row(
            "SELECT state FROM storage_nodes WHERE node_id = 'file-a'",
            [],
            |row| row.get(0),
        )
        .expect("original node state");
    assert_eq!(
        state, "trashed",
        "failed restore must leave the node recoverable"
    );
}

#[test]
fn interrupted_trash_journal_remains_discoverable_for_recovery() {
    let connection = workspace_connection();
    connection
        .execute(
            "INSERT INTO operation_journal (
                journal_id, operation_type, scope_id, node_id, transaction_id,
                phase, payload_json, state, created_at, updated_at
             ) VALUES (
                'journal-interrupted', 'trash_node', 'scope-a', 'file-a', NULL,
                'database_move',
                '{\"original_parent_node_id\":\"uploaded-a\",\"trash_node_id\":\"trash-a\"}',
                'recovery_required', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
             )",
            [],
        )
        .expect("insert interrupted journal");

    let recoverable: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM operation_journal
             WHERE state NOT IN ('committed', 'rolled_back')",
            [],
            |row| row.get(0),
        )
        .expect("recovery queue");
    assert_eq!(recoverable, 1);
}
