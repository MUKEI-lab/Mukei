//! SQLCipher-backed Universal Storage and workspace bootstrap repository.
//!
//! Scope creation is idempotent and transaction-bound. Existing scopes are
//! validated before being returned so a partially-created or tampered layout
//! cannot silently enter the runtime.

use crate::error::{MukeiError, Result};
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use crate::storage::universal::{
    ChatId, StorageNodeId, StorageScopeId, SystemDirectoryRole, WorkspaceId, WorkspaceLayout,
    UNIVERSAL_STORAGE_NAME,
};
use rusqlite::{OptionalExtension, TransactionBehavior};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedSystemDirectory {
    pub node_id: StorageNodeId,
    pub parent_node_id: Option<StorageNodeId>,
    pub role: SystemDirectoryRole,
    pub display_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedUniversalStorage {
    pub scope_id: StorageScopeId,
    pub root_node_id: StorageNodeId,
    pub directories: Vec<PersistedSystemDirectory>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedWorkspace {
    pub workspace_id: WorkspaceId,
    pub scope_id: StorageScopeId,
    pub chat_id: ChatId,
    pub root_node_id: StorageNodeId,
    pub directories: Vec<PersistedSystemDirectory>,
}

impl PersistedWorkspace {
    pub fn directory(&self, role: SystemDirectoryRole) -> Option<&PersistedSystemDirectory> {
        self.directories.iter().find(|entry| entry.role == role)
    }

    pub fn uploaded_files_node_id(&self) -> StorageNodeId {
        self.directory(SystemDirectoryRole::UploadedFiles)
            .expect("validated workspaces always contain Uploaded files")
            .node_id
    }
}

pub struct UniversalStorageRepository;

impl UniversalStorageRepository {
    /// Ensure the singleton Universal Storage root and its mandatory Trash
    /// directory exist. Existing layouts are validated before being returned.
    pub async fn ensure_universal_storage(
        pool: &DatabasePool,
    ) -> Result<PersistedUniversalStorage> {
        pool.with_conn(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

            let existing: Option<(String, String)> = transaction
                .query_row(
                    "SELECT scope_id, root_node_id FROM storage_scopes \
                     WHERE scope_type = 'universal' AND state != 'deleted' LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;

            let (scope_id, root_node_id) = if let Some((scope_id, root_node_id)) = existing {
                (parse_scope_id(scope_id)?, parse_node_id(root_node_id)?)
            } else {
                let scope_id = StorageScopeId::new();
                let root_node_id = StorageNodeId::new();
                let trash_node_id = StorageNodeId::new();
                let now = chrono::Utc::now().to_rfc3339();

                transaction.execute(
                    "INSERT INTO storage_scopes \
                        (scope_id, scope_type, owner_chat_id, root_node_id, display_name, state, created_at, updated_at) \
                     VALUES (?1, 'universal', NULL, ?2, ?3, 'active', ?4, ?4)",
                    rusqlite::params![
                        scope_id.to_string(),
                        root_node_id.to_string(),
                        UNIVERSAL_STORAGE_NAME,
                        &now,
                    ],
                )?;
                insert_system_directory(
                    &transaction,
                    scope_id,
                    root_node_id,
                    None,
                    SystemDirectoryRole::ScopeRoot,
                    UNIVERSAL_STORAGE_NAME,
                    &now,
                )?;
                insert_system_directory(
                    &transaction,
                    scope_id,
                    trash_node_id,
                    Some(root_node_id),
                    SystemDirectoryRole::Trash,
                    SystemDirectoryRole::Trash.display_name(),
                    &now,
                )?;
                (scope_id, root_node_id)
            };

            let directories = load_system_directories(&transaction, scope_id)?;
            validate_universal_layout(root_node_id, &directories)?;
            transaction.commit()?;

            Ok::<_, DbError>(PersistedUniversalStorage {
                scope_id,
                root_node_id,
                directories,
            })
        })
        .await
    }

    /// Ensure exactly one workspace exists for `chat_id`. Creation of the scope
    /// and all mandatory root directories is atomic.
    pub async fn ensure_workspace(
        pool: &DatabasePool,
        chat_id: ChatId,
    ) -> Result<PersistedWorkspace> {
        pool.with_conn(move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

            let existing: Option<(String, String, String)> = transaction
                .query_row(
                    "SELECT scope_id, root_node_id, display_name FROM storage_scopes \
                     WHERE scope_type = 'workspace' AND owner_chat_id = ?1 AND state != 'deleted' LIMIT 1",
                    [chat_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;

            let (workspace_id, scope_id, root_node_id) =
                if let Some((scope_id, root_node_id, display_name)) = existing {
                    let workspace_id = parse_workspace_display_id(&display_name)?;
                    (
                        workspace_id,
                        parse_scope_id(scope_id)?,
                        parse_node_id(root_node_id)?,
                    )
                } else {
                    let plan = WorkspaceLayout::plan(chat_id.clone());
                    let now = chrono::Utc::now().to_rfc3339();
                    let workspace_display_id = workspace_display_id(plan.workspace_id);

                    transaction.execute(
                        "INSERT INTO storage_scopes \
                            (scope_id, scope_type, owner_chat_id, root_node_id, display_name, state, created_at, updated_at) \
                         VALUES (?1, 'workspace', ?2, ?3, ?4, 'active', ?5, ?5)",
                        rusqlite::params![
                            plan.scope_id.to_string(),
                            plan.chat_id.as_str(),
                            plan.root_node_id.to_string(),
                            workspace_display_id,
                            &now,
                        ],
                    )?;

                    for directory in &plan.directories {
                        insert_system_directory(
                            &transaction,
                            plan.scope_id,
                            directory.node_id,
                            directory.parent_node_id,
                            directory.role,
                            directory.display_name,
                            &now,
                        )?;
                    }

                    (plan.workspace_id, plan.scope_id, plan.root_node_id)
                };

            let directories = load_system_directories(&transaction, scope_id)?;
            validate_workspace_layout(root_node_id, &directories)?;
            transaction.commit()?;

            Ok::<_, DbError>(PersistedWorkspace {
                workspace_id,
                scope_id,
                chat_id,
                root_node_id,
                directories,
            })
        })
        .await
    }
}

fn insert_system_directory(
    transaction: &rusqlite::Transaction<'_>,
    scope_id: StorageScopeId,
    node_id: StorageNodeId,
    parent_node_id: Option<StorageNodeId>,
    role: SystemDirectoryRole,
    display_name: &str,
    now: &str,
) -> std::result::Result<(), DbError> {
    transaction.execute(
        "INSERT INTO storage_nodes \
            (node_id, scope_id, parent_node_id, node_type, display_name, normalized_name, \
             current_version_id, system_role, state, created_at, updated_at, trashed_at) \
         VALUES (?1, ?2, ?3, 'directory', ?4, ?5, NULL, ?6, 'active', ?7, ?7, NULL)",
        rusqlite::params![
            node_id.to_string(),
            scope_id.to_string(),
            parent_node_id.map(|id| id.to_string()),
            display_name,
            display_name.to_ascii_lowercase(),
            role_as_str(role),
            now,
        ],
    )?;
    Ok(())
}

fn load_system_directories(
    transaction: &rusqlite::Transaction<'_>,
    scope_id: StorageScopeId,
) -> std::result::Result<Vec<PersistedSystemDirectory>, DbError> {
    let mut statement = transaction.prepare(
        "SELECT node_id, parent_node_id, system_role, display_name \
         FROM storage_nodes \
         WHERE scope_id = ?1 AND system_role IS NOT NULL AND state != 'deleted' \
         ORDER BY system_role, node_id",
    )?;
    let rows = statement
        .query_map([scope_id.to_string()], |row| {
            let node_id: String = row.get(0)?;
            let parent_node_id: Option<String> = row.get(1)?;
            let role: String = row.get(2)?;
            let display_name: String = row.get(3)?;
            Ok((node_id, parent_node_id, role, display_name))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    rows.into_iter()
        .map(|(node_id, parent_node_id, role, display_name)| {
            Ok(PersistedSystemDirectory {
                node_id: parse_node_id(node_id)?,
                parent_node_id: parent_node_id.map(parse_node_id).transpose()?,
                role: role_from_str(&role)?,
                display_name,
            })
        })
        .collect()
}

fn validate_universal_layout(
    root_node_id: StorageNodeId,
    directories: &[PersistedSystemDirectory],
) -> std::result::Result<(), DbError> {
    validate_role_once(directories, SystemDirectoryRole::ScopeRoot)?;
    validate_role_once(directories, SystemDirectoryRole::Trash)?;
    if directories.len() != 2 {
        return invariant("Universal Storage contains an unexpected system directory set");
    }
    validate_root_and_children(root_node_id, directories)
}

fn validate_workspace_layout(
    root_node_id: StorageNodeId,
    directories: &[PersistedSystemDirectory],
) -> std::result::Result<(), DbError> {
    validate_role_once(directories, SystemDirectoryRole::ScopeRoot)?;
    for role in SystemDirectoryRole::WORKSPACE_CHILDREN {
        validate_role_once(directories, role)?;
    }
    if directories.len() != 8 {
        return invariant("workspace contains an unexpected system directory set");
    }
    validate_root_and_children(root_node_id, directories)
}

fn validate_root_and_children(
    root_node_id: StorageNodeId,
    directories: &[PersistedSystemDirectory],
) -> std::result::Result<(), DbError> {
    let root = directories
        .iter()
        .find(|entry| entry.role == SystemDirectoryRole::ScopeRoot)
        .ok_or_else(|| DbError::Domain(MukeiError::Invariant("scope root is missing".into())))?;
    if root.node_id != root_node_id || root.parent_node_id.is_some() {
        return invariant("scope root linkage is invalid");
    }
    if directories
        .iter()
        .filter(|entry| entry.role != SystemDirectoryRole::ScopeRoot)
        .any(|entry| entry.parent_node_id != Some(root_node_id))
    {
        return invariant("system directory is not a direct child of the scope root");
    }
    Ok(())
}

fn validate_role_once(
    directories: &[PersistedSystemDirectory],
    role: SystemDirectoryRole,
) -> std::result::Result<(), DbError> {
    if directories.iter().filter(|entry| entry.role == role).count() != 1 {
        return invariant(format!("system directory role {role:?} is missing or duplicated"));
    }
    Ok(())
}

fn parse_scope_id(value: String) -> std::result::Result<StorageScopeId, DbError> {
    Ok(StorageScopeId(parse_uuid("scope_id", &value)?))
}

fn parse_node_id(value: String) -> std::result::Result<StorageNodeId, DbError> {
    Ok(StorageNodeId(parse_uuid("node_id", &value)?))
}

fn parse_uuid(field: &str, value: &str) -> std::result::Result<Uuid, DbError> {
    Uuid::parse_str(value).map_err(|_| {
        DbError::Domain(MukeiError::Invariant(format!(
            "stored {field} is not a valid UUID"
        )))
    })
}

fn workspace_display_id(workspace_id: WorkspaceId) -> String {
    format!("workspace:{}", workspace_id)
}

fn parse_workspace_display_id(value: &str) -> std::result::Result<WorkspaceId, DbError> {
    let encoded = value
        .strip_prefix("workspace:")
        .ok_or_else(|| DbError::Domain(MukeiError::Invariant("workspace identifier is missing".into())))?;
    Ok(WorkspaceId(parse_uuid("workspace_id", encoded)?))
}

fn role_as_str(role: SystemDirectoryRole) -> &'static str {
    match role {
        SystemDirectoryRole::ScopeRoot => "scope_root",
        SystemDirectoryRole::UploadedFiles => "uploaded_files",
        SystemDirectoryRole::GeneratedFiles => "generated_files",
        SystemDirectoryRole::Drafts => "drafts",
        SystemDirectoryRole::Research => "research",
        SystemDirectoryRole::Exports => "exports",
        SystemDirectoryRole::Temporary => "temporary",
        SystemDirectoryRole::Trash => "trash",
    }
}

fn role_from_str(value: &str) -> std::result::Result<SystemDirectoryRole, DbError> {
    match value {
        "scope_root" => Ok(SystemDirectoryRole::ScopeRoot),
        "uploaded_files" => Ok(SystemDirectoryRole::UploadedFiles),
        "generated_files" => Ok(SystemDirectoryRole::GeneratedFiles),
        "drafts" => Ok(SystemDirectoryRole::Drafts),
        "research" => Ok(SystemDirectoryRole::Research),
        "exports" => Ok(SystemDirectoryRole::Exports),
        "temporary" => Ok(SystemDirectoryRole::Temporary),
        "trash" => Ok(SystemDirectoryRole::Trash),
        _ => invariant("unknown system directory role"),
    }
}

fn invariant<T>(message: impl Into<String>) -> std::result::Result<T, DbError> {
    Err(DbError::Domain(MukeiError::Invariant(message.into())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrations::Migrator;

    async fn migrated_pool() -> (tempfile::TempDir, DatabasePool) {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("storage.db");
        let pool = DatabasePool::open(&database_path).unwrap();
        Migrator::embedded().apply_pending(&pool).await.unwrap();
        (directory, pool)
    }

    #[tokio::test]
    async fn universal_storage_creation_is_idempotent() {
        let (_directory, pool) = migrated_pool().await;
        let first = UniversalStorageRepository::ensure_universal_storage(&pool)
            .await
            .unwrap();
        let second = UniversalStorageRepository::ensure_universal_storage(&pool)
            .await
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.directories.len(), 2);
    }

    #[tokio::test]
    async fn workspace_creation_is_atomic_and_idempotent() {
        let (_directory, pool) = migrated_pool().await;
        let chat_id = ChatId::parse("chat-1").unwrap();
        let first = UniversalStorageRepository::ensure_workspace(&pool, chat_id.clone())
            .await
            .unwrap();
        let second = UniversalStorageRepository::ensure_workspace(&pool, chat_id)
            .await
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.directories.len(), 8);
        assert!(first.directory(SystemDirectoryRole::UploadedFiles).is_some());
    }

    #[tokio::test]
    async fn different_chats_receive_isolated_workspaces() {
        let (_directory, pool) = migrated_pool().await;
        let first = UniversalStorageRepository::ensure_workspace(
            &pool,
            ChatId::parse("chat-1").unwrap(),
        )
        .await
        .unwrap();
        let second = UniversalStorageRepository::ensure_workspace(
            &pool,
            ChatId::parse("chat-2").unwrap(),
        )
        .await
        .unwrap();

        assert_ne!(first.workspace_id, second.workspace_id);
        assert_ne!(first.scope_id, second.scope_id);
        assert_ne!(first.uploaded_files_node_id(), second.uploaded_files_node_id());
    }
}
