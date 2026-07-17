#![cfg(feature = "rusqlite")]

use std::path::PathBuf;

use mukei_core::storage::{
    admit_file_name, ChatId, DatabasePool, DbError, DuplicatePolicy, ImportAuthorization,
    ImportCommitRepository, ImportCommitRequest, ImportJournalRepository, ImportState,
    PooledConnectionExt, StorageObjectId, StoredObject, UniversalStorageRepository,
    WorkspaceAccessContext,
};

const V014: &str = include_str!("../../../migrations/V014__universal_storage_and_workspaces.sql");
const V015: &str = include_str!("../../../migrations/V015__workspace_scope_isolation_guards.sql");

#[tokio::test]
async fn committed_retry_requires_the_original_workspace_authorization() {
    let directory = tempfile::tempdir().unwrap();
    let pool = DatabasePool::open(&directory.path().join("storage.db")).unwrap();
    pool.with_conn(|connection| {
        connection.execute_batch(V014)?;
        connection.execute_batch(V015)?;
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();

    let owner = UniversalStorageRepository::ensure_workspace(
        &pool,
        ChatId::parse("chat-owner").unwrap(),
    )
    .await
    .unwrap();
    let outsider = UniversalStorageRepository::ensure_workspace(
        &pool,
        ChatId::parse("chat-outsider").unwrap(),
    )
    .await
    .unwrap();

    let transaction_id = ImportJournalRepository::create(
        &pool,
        owner.scope_id,
        owner.uploaded_files_node_id(),
        "notes.md".into(),
        "retry-isolation.partial".into(),
        Some(4),
        None,
    )
    .await
    .unwrap();
    for state in [
        ImportState::Validating,
        ImportState::Copying,
        ImportState::Hashing,
        ImportState::Encrypting,
        ImportState::CommittingObject,
        ImportState::CommittingNode,
    ] {
        ImportJournalRepository::transition(&pool, transaction_id, state, None, None)
            .await
            .unwrap();
    }

    let digest = [41_u8; 32];
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let stored_object = StoredObject {
        object_id: StorageObjectId::new(),
        plaintext_sha256: digest,
        plaintext_size: 4,
        encrypted_size: 48,
        relative_path: PathBuf::from(format!(
            "{}/{}/{}-4.mobj",
            &hex[0..2],
            &hex[2..4],
            hex
        )),
        deduplicated: false,
    };
    let owner_access = WorkspaceAccessContext {
        chat_id: owner.chat_id.clone(),
        workspace_id: owner.workspace_id,
    };
    let request = ImportCommitRequest {
        transaction_id,
        authorization: ImportAuthorization::Workspace(owner_access),
        admitted_name: admit_file_name("notes.md").unwrap(),
        stored_object,
        detected_format: "markdown".into(),
        detected_mime: Some("text/markdown".into()),
        detected_encoding: Some("utf-8".into()),
        language_id: Some("markdown".into()),
        encryption_version: 1,
        duplicate_policy: DuplicatePolicy::RenameNewEntry,
    };

    let committed = ImportCommitRepository::commit(&pool, request.clone())
        .await
        .unwrap();

    let mut unauthorized_retry = request.clone();
    unauthorized_retry.authorization = ImportAuthorization::Workspace(WorkspaceAccessContext {
        chat_id: outsider.chat_id.clone(),
        workspace_id: outsider.workspace_id,
    });
    assert!(ImportCommitRepository::commit(&pool, unauthorized_retry)
        .await
        .is_err());

    let authorized_retry = ImportCommitRepository::commit(&pool, request)
        .await
        .unwrap();
    assert_eq!(authorized_retry.node_id, committed.node_id);
    assert_eq!(authorized_retry.object_id, committed.object_id);
    assert_eq!(authorized_retry.version_id, committed.version_id);

    pool.with_conn(move |connection| {
        let file_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM storage_nodes WHERE node_type = 'file'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(file_count, 1);
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();
}
