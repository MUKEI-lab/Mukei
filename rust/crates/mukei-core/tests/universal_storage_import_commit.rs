#![cfg(feature = "rusqlite")]

use std::path::PathBuf;

use mukei_core::storage::{
    admit_file_name, ChatId, DatabasePool, DbError, DuplicatePolicy, ImportAuthorization,
    ImportCommitRepository, ImportCommitRequest, ImportJournalRepository, ImportState,
    PersistedWorkspace, PooledConnectionExt, StorageObjectId, StoredObject,
    UniversalStorageRepository, WorkspaceAccessContext,
};

const V014: &str = include_str!("../../../migrations/V014__universal_storage_and_workspaces.sql");
const V015: &str = include_str!("../../../migrations/V015__workspace_scope_isolation_guards.sql");

async fn migrated_pool() -> (tempfile::TempDir, DatabasePool) {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let database_path = directory.path().join("storage.db");
    let pool = DatabasePool::open(&database_path).expect("open database pool");
    pool.with_conn(|connection| {
        connection.execute_batch(V014)?;
        connection.execute_batch(V015)?;
        Ok::<_, DbError>(())
    })
    .await
    .expect("apply storage migrations");
    (directory, pool)
}

fn access(workspace: &PersistedWorkspace) -> WorkspaceAccessContext {
    WorkspaceAccessContext {
        chat_id: workspace.chat_id.clone(),
        workspace_id: workspace.workspace_id,
    }
}

async fn import_ready_for_node_commit(
    pool: &DatabasePool,
    workspace: &PersistedWorkspace,
    filename: &str,
    staging_name: &str,
) -> mukei_core::storage::ImportTransactionId {
    let transaction_id = ImportJournalRepository::create(
        pool,
        workspace.scope_id,
        workspace.uploaded_files_node_id(),
        filename.to_owned(),
        staging_name.to_owned(),
        Some(12),
        Some(format!("source:{staging_name}")),
    )
    .await
    .expect("create import transaction");

    for state in [
        ImportState::Validating,
        ImportState::Copying,
        ImportState::Hashing,
        ImportState::Encrypting,
        ImportState::CommittingObject,
        ImportState::CommittingNode,
    ] {
        ImportJournalRepository::transition(pool, transaction_id, state, None, None)
            .await
            .expect("advance import transaction");
    }
    transaction_id
}

fn stored_object(seed: u8, object_id: StorageObjectId) -> StoredObject {
    let digest = [seed; 32];
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    StoredObject {
        object_id,
        plaintext_sha256: digest,
        plaintext_size: 12,
        encrypted_size: 64,
        relative_path: PathBuf::from(format!(
            "{}/{}/{}-12.mobj",
            &hex[0..2],
            &hex[2..4],
            hex
        )),
        deduplicated: false,
    }
}

fn commit_request(
    transaction_id: mukei_core::storage::ImportTransactionId,
    workspace: &PersistedWorkspace,
    filename: &str,
    object: StoredObject,
    duplicate_policy: DuplicatePolicy,
) -> ImportCommitRequest {
    ImportCommitRequest {
        transaction_id,
        authorization: ImportAuthorization::Workspace(access(workspace)),
        admitted_name: admit_file_name(filename).expect("allowed test filename"),
        stored_object: object,
        detected_format: "markdown".into(),
        detected_mime: Some("text/markdown".into()),
        detected_encoding: Some("utf-8".into()),
        language_id: Some("markdown".into()),
        encryption_version: 1,
        duplicate_policy,
    }
}

#[tokio::test]
async fn first_publish_is_atomic_and_retry_is_idempotent() {
    let (_directory, pool) = migrated_pool().await;
    let workspace = UniversalStorageRepository::ensure_workspace(
        &pool,
        ChatId::parse("chat-a").unwrap(),
    )
    .await
    .expect("workspace");
    let transaction_id =
        import_ready_for_node_commit(&pool, &workspace, "notes.md", "txn-a.partial").await;
    let request = commit_request(
        transaction_id,
        &workspace,
        "notes.md",
        stored_object(7, StorageObjectId::new()),
        DuplicatePolicy::RenameNewEntry,
    );

    let first = ImportCommitRepository::commit(&pool, request.clone())
        .await
        .expect("publish imported file");
    assert_eq!(first.display_name, "notes.md");
    assert!(!first.reused_object);
    assert!(!first.reused_version);

    let retry = ImportCommitRepository::commit(&pool, request)
        .await
        .expect("idempotent retry");
    assert_eq!(retry.node_id, first.node_id);
    assert_eq!(retry.version_id, first.version_id);
    assert_eq!(retry.object_id, first.object_id);

    pool.with_conn(move |connection| {
        let counts: (i64, i64, i64) = connection.query_row(
            "SELECT \
                (SELECT COUNT(*) FROM storage_objects), \
                (SELECT COUNT(*) FROM file_versions), \
                (SELECT COUNT(*) FROM storage_nodes WHERE node_type = 'file')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(counts, (1, 1, 1));
        let state: String = connection.query_row(
            "SELECT state FROM import_transactions WHERE transaction_id = ?1",
            [transaction_id.to_string()],
            |row| row.get(0),
        )?;
        assert_eq!(state, "indexing");
        let journal_state: String = connection.query_row(
            "SELECT state FROM operation_journal WHERE transaction_id = ?1",
            [transaction_id.to_string()],
            |row| row.get(0),
        )?;
        assert_eq!(journal_state, "committed");
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn identical_content_reuses_object_and_version_but_allocates_a_safe_name() {
    let (_directory, pool) = migrated_pool().await;
    let workspace = UniversalStorageRepository::ensure_workspace(
        &pool,
        ChatId::parse("chat-a").unwrap(),
    )
    .await
    .expect("workspace");

    let first_tx =
        import_ready_for_node_commit(&pool, &workspace, "notes.md", "txn-first.partial").await;
    let first = ImportCommitRepository::commit(
        &pool,
        commit_request(
            first_tx,
            &workspace,
            "notes.md",
            stored_object(9, StorageObjectId::new()),
            DuplicatePolicy::RenameNewEntry,
        ),
    )
    .await
    .expect("first import");

    let second_tx =
        import_ready_for_node_commit(&pool, &workspace, "notes.md", "txn-second.partial").await;
    let mut duplicate = stored_object(9, StorageObjectId::new());
    duplicate.deduplicated = true;
    let second = ImportCommitRepository::commit(
        &pool,
        commit_request(
            second_tx,
            &workspace,
            "notes.md",
            duplicate,
            DuplicatePolicy::RenameNewEntry,
        ),
    )
    .await
    .expect("deduplicated import");

    assert_eq!(second.display_name, "notes (2).md");
    assert_eq!(second.object_id, first.object_id);
    assert_eq!(second.version_id, first.version_id);
    assert!(second.reused_object);
    assert!(second.reused_version);
    assert_ne!(second.node_id, first.node_id);

    pool.with_conn(|connection| {
        let counts: (i64, i64, i64) = connection.query_row(
            "SELECT \
                (SELECT COUNT(*) FROM storage_objects), \
                (SELECT COUNT(*) FROM file_versions), \
                (SELECT COUNT(*) FROM storage_nodes WHERE node_type = 'file')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(counts, (1, 1, 2));
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn cross_chat_authorization_is_rejected_before_a_recovery_journal_is_created() {
    let (_directory, pool) = migrated_pool().await;
    let first = UniversalStorageRepository::ensure_workspace(
        &pool,
        ChatId::parse("chat-a").unwrap(),
    )
    .await
    .unwrap();
    let second = UniversalStorageRepository::ensure_workspace(
        &pool,
        ChatId::parse("chat-b").unwrap(),
    )
    .await
    .unwrap();
    let transaction_id =
        import_ready_for_node_commit(&pool, &second, "private.md", "txn-private.partial").await;
    let mut request = commit_request(
        transaction_id,
        &second,
        "private.md",
        stored_object(11, StorageObjectId::new()),
        DuplicatePolicy::RenameNewEntry,
    );
    request.authorization = ImportAuthorization::Workspace(access(&first));

    assert!(ImportCommitRepository::commit(&pool, request).await.is_err());
    pool.with_conn(move |connection| {
        let journal_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM operation_journal WHERE transaction_id = ?1",
            [transaction_id.to_string()],
            |row| row.get(0),
        )?;
        let object_count: i64 =
            connection.query_row("SELECT COUNT(*) FROM storage_objects", [], |row| row.get(0))?;
        let state: String = connection.query_row(
            "SELECT state FROM import_transactions WHERE transaction_id = ?1",
            [transaction_id.to_string()],
            |row| row.get(0),
        )?;
        assert_eq!(journal_count, 0);
        assert_eq!(object_count, 0);
        assert_eq!(state, "committing_node");
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn rejected_name_conflict_retains_filesystem_recovery_evidence() {
    let (_directory, pool) = migrated_pool().await;
    let workspace = UniversalStorageRepository::ensure_workspace(
        &pool,
        ChatId::parse("chat-a").unwrap(),
    )
    .await
    .unwrap();

    let first_tx =
        import_ready_for_node_commit(&pool, &workspace, "notes.md", "txn-a.partial").await;
    ImportCommitRepository::commit(
        &pool,
        commit_request(
            first_tx,
            &workspace,
            "notes.md",
            stored_object(13, StorageObjectId::new()),
            DuplicatePolicy::RenameNewEntry,
        ),
    )
    .await
    .unwrap();

    let second_tx =
        import_ready_for_node_commit(&pool, &workspace, "notes.md", "txn-b.partial").await;
    let result = ImportCommitRepository::commit(
        &pool,
        commit_request(
            second_tx,
            &workspace,
            "notes.md",
            stored_object(14, StorageObjectId::new()),
            DuplicatePolicy::RejectNameConflict,
        ),
    )
    .await;
    assert!(result.is_err());

    pool.with_conn(move |connection| {
        let object_count: i64 =
            connection.query_row("SELECT COUNT(*) FROM storage_objects", [], |row| row.get(0))?;
        let journal_state: String = connection.query_row(
            "SELECT state FROM operation_journal WHERE transaction_id = ?1",
            [second_tx.to_string()],
            |row| row.get(0),
        )?;
        let import_state: String = connection.query_row(
            "SELECT state FROM import_transactions WHERE transaction_id = ?1",
            [second_tx.to_string()],
            |row| row.get(0),
        )?;
        assert_eq!(object_count, 1, "failed DB publication must roll back the new object row");
        assert_eq!(journal_state, "applied_filesystem");
        assert_eq!(import_state, "committing_node");
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();
}
