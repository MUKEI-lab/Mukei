#![cfg(feature = "rusqlite")]

use std::sync::Arc;

use mukei_core::storage::{
    DatabasePool, Migrator, SqlStorageWorkspaceService, StorageNodeId, StorageWorkspacePort,
};
use uuid::Uuid;

async fn workspace_service() -> (
    tempfile::TempDir,
    SqlStorageWorkspaceService,
) {
    let directory = tempfile::tempdir().expect("temporary storage directory");
    let database_path = directory.path().join("mukei-storage.sqlite3");
    let pool = DatabasePool::open(&database_path).expect("open test database");
    Migrator::embedded()
        .apply_pending(&pool)
        .await
        .expect("apply embedded migrations");
    let service = SqlStorageWorkspaceService::new(Arc::new(pool));
    (directory, service)
}

fn node_id(value: &str) -> StorageNodeId {
    StorageNodeId(Uuid::parse_str(value).expect("storage node UUID"))
}

#[tokio::test]
async fn snapshot_exposes_root_and_protected_trash() {
    let (_directory, service) = workspace_service().await;
    let snapshot = service
        .universal_snapshot()
        .await
        .expect("Universal Storage snapshot");

    let root = snapshot
        .nodes
        .iter()
        .find(|node| node.node_id == snapshot.root_node_id)
        .expect("scope root node");
    assert_eq!(root.node_type, "directory");
    assert_eq!(root.system_role.as_deref(), Some("scope_root"));

    let trash = snapshot
        .nodes
        .iter()
        .find(|node| node.system_role.as_deref() == Some("trash"))
        .expect("Trash node");
    assert_eq!(trash.parent_node_id.as_deref(), Some(snapshot.root_node_id.as_str()));
    assert_eq!(trash.state, "active");
}

#[tokio::test]
async fn directory_creation_is_nested_and_name_conflicts_fail_closed() {
    let (_directory, service) = workspace_service().await;
    let snapshot = service.universal_snapshot().await.expect("initial snapshot");
    let root = node_id(&snapshot.root_node_id);

    let parent = service
        .create_directory(root, "Research".to_owned())
        .await
        .expect("create parent directory");
    let child = service
        .create_directory(node_id(&parent.node_id), "Sources".to_owned())
        .await
        .expect("create nested directory");

    assert_eq!(parent.parent_node_id.as_deref(), Some(snapshot.root_node_id.as_str()));
    assert_eq!(child.parent_node_id.as_deref(), Some(parent.node_id.as_str()));
    assert!(service
        .create_directory(root, "research".to_owned())
        .await
        .is_err());
}

#[tokio::test]
async fn system_directories_cannot_be_renamed_or_trashed() {
    let (_directory, service) = workspace_service().await;
    let snapshot = service.universal_snapshot().await.expect("initial snapshot");
    let root = node_id(&snapshot.root_node_id);
    let trash = snapshot
        .nodes
        .iter()
        .find(|node| node.system_role.as_deref() == Some("trash"))
        .expect("Trash node");

    assert!(service
        .rename_node(root, "Renamed root".to_owned())
        .await
        .is_err());
    assert!(service.trash_node(node_id(&trash.node_id)).await.is_err());
}

#[tokio::test]
async fn trash_and_restore_preserve_identity_and_original_parent() {
    let (_directory, service) = workspace_service().await;
    let snapshot = service.universal_snapshot().await.expect("initial snapshot");
    let root = node_id(&snapshot.root_node_id);
    let trash = snapshot
        .nodes
        .iter()
        .find(|node| node.system_role.as_deref() == Some("trash"))
        .expect("Trash node")
        .clone();

    let parent = service
        .create_directory(root, "Archive".to_owned())
        .await
        .expect("create parent");
    let child = service
        .create_directory(node_id(&parent.node_id), "Drafts".to_owned())
        .await
        .expect("create child");
    let original_id = child.node_id.clone();

    let trashed = service
        .trash_node(node_id(&child.node_id))
        .await
        .expect("move to Trash");
    assert_eq!(trashed.node_id, original_id);
    assert_eq!(trashed.state, "trashed");
    assert_eq!(trashed.parent_node_id.as_deref(), Some(trash.node_id.as_str()));

    let restored = service
        .restore_node(node_id(&trashed.node_id))
        .await
        .expect("restore node");
    assert_eq!(restored.node_id, original_id);
    assert_eq!(restored.state, "active");
    assert_eq!(restored.parent_node_id.as_deref(), Some(parent.node_id.as_str()));
}
