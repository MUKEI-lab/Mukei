//! SQLCipher-backed user-facing Universal Storage workspace operations.
//!
//! Logical directory entries live only in `storage_nodes`; file bytes remain in
//! the immutable encrypted object store. Mutations fail closed across scopes and
//! protect system-owned root/Trash identities.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::error::{MukeiError, Result};
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use crate::storage::universal::{StorageNodeId, StorageScopeId, SystemDirectoryRole};
use crate::storage::universal_repository::UniversalStorageRepository;

const MAX_DIRECTORY_NAME_BYTES: usize = 255;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageNodeSnapshot {
    pub node_id: String,
    pub parent_node_id: Option<String>,
    pub node_type: String,
    pub display_name: String,
    pub state: String,
    pub system_role: Option<String>,
    pub size_bytes: Option<u64>,
    pub mime_type: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniversalStorageSnapshot {
    pub scope_id: String,
    pub root_node_id: String,
    pub nodes: Vec<StorageNodeSnapshot>,
}

#[async_trait]
pub trait StorageWorkspacePort: Send + Sync {
    async fn universal_snapshot(&self) -> Result<UniversalStorageSnapshot>;
    async fn create_directory(
        &self,
        parent_node_id: StorageNodeId,
        display_name: String,
    ) -> Result<StorageNodeSnapshot>;
    async fn rename_node(
        &self,
        node_id: StorageNodeId,
        display_name: String,
    ) -> Result<StorageNodeSnapshot>;
    async fn trash_node(&self, node_id: StorageNodeId) -> Result<StorageNodeSnapshot>;
    async fn restore_node(&self, node_id: StorageNodeId) -> Result<StorageNodeSnapshot>;
}

pub struct SqlStorageWorkspaceService {
    pool: Arc<DatabasePool>,
}

impl SqlStorageWorkspaceService {
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }

    async fn load_node(&self, node_id: StorageNodeId) -> Result<StorageNodeSnapshot> {
        self.pool
            .with_conn(move |connection| {
                load_node_snapshot(connection, node_id.to_string())?
                    .ok_or_else(|| invariant("storage node was not found"))
            })
            .await
    }
}

#[async_trait]
impl StorageWorkspacePort for SqlStorageWorkspaceService {
    async fn universal_snapshot(&self) -> Result<UniversalStorageSnapshot> {
        let universal = UniversalStorageRepository::ensure_universal_storage(&self.pool).await?;
        let scope_id = universal.scope_id;
        let root_node_id = universal.root_node_id;
        let nodes = self
            .pool
            .with_conn(move |connection| {
                let mut statement = connection.prepare(
                    "SELECT n.node_id, n.parent_node_id, n.node_type, n.display_name, n.state, \
                            n.system_role, o.plaintext_size, o.detected_mime, n.updated_at \
                     FROM storage_nodes n \
                     LEFT JOIN file_versions v ON v.version_id = n.current_version_id \
                     LEFT JOIN storage_objects o ON o.object_id = v.object_id \
                     WHERE n.scope_id = ?1 AND n.state != 'deleted' \
                     ORDER BY CASE n.node_type WHEN 'directory' THEN 0 ELSE 1 END, \
                              n.normalized_name, n.node_id",
                )?;
                let rows = statement
                    .query_map([scope_id.to_string()], snapshot_from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok::<_, DbError>(rows)
            })
            .await?;
        Ok(UniversalStorageSnapshot {
            scope_id: scope_id.to_string(),
            root_node_id: root_node_id.to_string(),
            nodes,
        })
    }

    async fn create_directory(
        &self,
        parent_node_id: StorageNodeId,
        display_name: String,
    ) -> Result<StorageNodeSnapshot> {
        let display_name = validate_user_name(&display_name)?;
        let normalized_name = display_name.to_ascii_lowercase();
        let universal = UniversalStorageRepository::ensure_universal_storage(&self.pool).await?;
        let scope_id = universal.scope_id;
        let node_id = StorageNodeId::new();
        let node_id_for_db = node_id;
        self.pool
            .with_conn(move |connection| {
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                validate_parent_directory(
                    &transaction,
                    scope_id,
                    parent_node_id,
                    false,
                )?;
                reject_sibling_conflict(
                    &transaction,
                    scope_id,
                    parent_node_id,
                    &normalized_name,
                    None,
                )?;
                let now = chrono::Utc::now().to_rfc3339();
                transaction.execute(
                    "INSERT INTO storage_nodes \
                        (node_id, scope_id, parent_node_id, node_type, display_name, normalized_name, \
                         current_version_id, system_role, state, created_at, updated_at, trashed_at) \
                     VALUES (?1, ?2, ?3, 'directory', ?4, ?5, NULL, NULL, 'active', ?6, ?6, NULL)",
                    rusqlite::params![
                        node_id_for_db.to_string(),
                        scope_id.to_string(),
                        parent_node_id.to_string(),
                        display_name,
                        normalized_name,
                        now,
                    ],
                )?;
                transaction.commit()?;
                Ok::<_, DbError>(())
            })
            .await?;
        self.load_node(node_id).await
    }

    async fn rename_node(
        &self,
        node_id: StorageNodeId,
        display_name: String,
    ) -> Result<StorageNodeSnapshot> {
        let display_name = validate_user_name(&display_name)?;
        let normalized_name = display_name.to_ascii_lowercase();
        let universal = UniversalStorageRepository::ensure_universal_storage(&self.pool).await?;
        let scope_id = universal.scope_id;
        self.pool
            .with_conn(move |connection| {
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let row: Option<(String, Option<String>, Option<String>, String)> = transaction
                    .query_row(
                        "SELECT scope_id, parent_node_id, system_role, state \
                         FROM storage_nodes WHERE node_id = ?1",
                        [node_id.to_string()],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .optional()?;
                let (stored_scope, parent_raw, system_role, state) =
                    row.ok_or_else(|| invariant("storage node was not found"))?;
                if stored_scope != scope_id.to_string() || state != "active" {
                    return Err(invariant("storage node is not active in Universal Storage"));
                }
                if system_role.is_some() {
                    return Err(invariant("system storage directories cannot be renamed"));
                }
                let parent_node_id = parent_raw
                    .ok_or_else(|| invariant("Universal Storage root cannot be renamed"))
                    .and_then(parse_node_id)?;
                reject_sibling_conflict(
                    &transaction,
                    scope_id,
                    parent_node_id,
                    &normalized_name,
                    Some(node_id),
                )?;
                let changed = transaction.execute(
                    "UPDATE storage_nodes SET display_name = ?1, normalized_name = ?2, updated_at = ?3 \
                     WHERE node_id = ?4 AND scope_id = ?5 AND state = 'active' AND system_role IS NULL",
                    rusqlite::params![
                        display_name,
                        normalized_name,
                        chrono::Utc::now().to_rfc3339(),
                        node_id.to_string(),
                        scope_id.to_string(),
                    ],
                )?;
                if changed != 1 {
                    return Err(invariant("storage node changed while renaming"));
                }
                transaction.commit()?;
                Ok::<_, DbError>(())
            })
            .await?;
        self.load_node(node_id).await
    }

    async fn trash_node(&self, node_id: StorageNodeId) -> Result<StorageNodeSnapshot> {
        let universal = UniversalStorageRepository::ensure_universal_storage(&self.pool).await?;
        let scope_id = universal.scope_id;
        let trash_node_id = universal
            .directories
            .iter()
            .find(|entry| entry.role == SystemDirectoryRole::Trash)
            .map(|entry| entry.node_id)
            .ok_or_else(|| MukeiError::Invariant("Universal Storage Trash is missing".into()))?;
        self.pool
            .with_conn(move |connection| {
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let row: Option<(String, Option<String>, Option<String>, String)> = transaction
                    .query_row(
                        "SELECT scope_id, parent_node_id, system_role, state \
                         FROM storage_nodes WHERE node_id = ?1",
                        [node_id.to_string()],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .optional()?;
                let (stored_scope, parent_raw, system_role, state) =
                    row.ok_or_else(|| invariant("storage node was not found"))?;
                if stored_scope != scope_id.to_string() || state != "active" {
                    return Err(invariant("only active Universal Storage nodes can be trashed"));
                }
                if system_role.is_some() {
                    return Err(invariant("system storage directories cannot be trashed"));
                }
                let original_parent = parent_raw
                    .ok_or_else(|| invariant("Universal Storage root cannot be trashed"))?;
                let journal_id = Uuid::new_v4();
                let now = chrono::Utc::now().to_rfc3339();
                let payload = json!({
                    "original_parent_node_id": original_parent,
                    "trash_node_id": trash_node_id.to_string(),
                })
                .to_string();
                transaction.execute(
                    "INSERT INTO operation_journal \
                        (journal_id, operation_type, scope_id, node_id, transaction_id, phase, payload_json, state, created_at, updated_at) \
                     VALUES (?1, 'trash_node', ?2, ?3, NULL, 'database_move', ?4, 'prepared', ?5, ?5)",
                    rusqlite::params![
                        journal_id.to_string(),
                        scope_id.to_string(),
                        node_id.to_string(),
                        payload,
                        now,
                    ],
                )?;
                let changed = transaction.execute(
                    "UPDATE storage_nodes \
                     SET parent_node_id = ?1, state = 'trashed', trashed_at = ?2, updated_at = ?2 \
                     WHERE node_id = ?3 AND scope_id = ?4 AND state = 'active' AND system_role IS NULL",
                    rusqlite::params![
                        trash_node_id.to_string(),
                        now,
                        node_id.to_string(),
                        scope_id.to_string(),
                    ],
                )?;
                if changed != 1 {
                    return Err(invariant("storage node changed while moving to Trash"));
                }
                transaction.execute(
                    "UPDATE operation_journal SET phase = 'complete', state = 'committed', updated_at = ?1 \
                     WHERE journal_id = ?2 AND state = 'prepared'",
                    rusqlite::params![now, journal_id.to_string()],
                )?;
                transaction.commit()?;
                Ok::<_, DbError>(())
            })
            .await?;
        self.load_node(node_id).await
    }

    async fn restore_node(&self, node_id: StorageNodeId) -> Result<StorageNodeSnapshot> {
        let universal = UniversalStorageRepository::ensure_universal_storage(&self.pool).await?;
        let scope_id = universal.scope_id;
        self.pool
            .with_conn(move |connection| {
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let row: Option<(String, String, String)> = transaction
                    .query_row(
                        "SELECT scope_id, normalized_name, state FROM storage_nodes WHERE node_id = ?1",
                        [node_id.to_string()],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .optional()?;
                let (stored_scope, normalized_name, state) =
                    row.ok_or_else(|| invariant("storage node was not found"))?;
                if stored_scope != scope_id.to_string() || state != "trashed" {
                    return Err(invariant("only trashed Universal Storage nodes can be restored"));
                }
                let parent_raw: Option<String> = transaction
                    .query_row(
                        "SELECT json_extract(payload_json, '$.original_parent_node_id') \
                         FROM operation_journal \
                         WHERE operation_type = 'trash_node' AND scope_id = ?1 AND node_id = ?2 AND state = 'committed' \
                         ORDER BY created_at DESC LIMIT 1",
                        rusqlite::params![scope_id.to_string(), node_id.to_string()],
                        |row| row.get(0),
                    )
                    .optional()?
                    .flatten();
                let parent_node_id = parent_raw
                    .ok_or_else(|| invariant("committed trash journal is missing original parent"))
                    .and_then(parse_node_id)?;
                validate_parent_directory(&transaction, scope_id, parent_node_id, false)?;
                reject_sibling_conflict(
                    &transaction,
                    scope_id,
                    parent_node_id,
                    &normalized_name,
                    Some(node_id),
                )?;
                let journal_id = Uuid::new_v4();
                let now = chrono::Utc::now().to_rfc3339();
                transaction.execute(
                    "INSERT INTO operation_journal \
                        (journal_id, operation_type, scope_id, node_id, transaction_id, phase, payload_json, state, created_at, updated_at) \
                     VALUES (?1, 'restore_node', ?2, ?3, NULL, 'database_move', ?4, 'prepared', ?5, ?5)",
                    rusqlite::params![
                        journal_id.to_string(),
                        scope_id.to_string(),
                        node_id.to_string(),
                        json!({"restored_parent_node_id": parent_node_id.to_string()}).to_string(),
                        now,
                    ],
                )?;
                let changed = transaction.execute(
                    "UPDATE storage_nodes \
                     SET parent_node_id = ?1, state = 'active', trashed_at = NULL, updated_at = ?2 \
                     WHERE node_id = ?3 AND scope_id = ?4 AND state = 'trashed'",
                    rusqlite::params![
                        parent_node_id.to_string(),
                        now,
                        node_id.to_string(),
                        scope_id.to_string(),
                    ],
                )?;
                if changed != 1 {
                    return Err(invariant("storage node changed while restoring"));
                }
                transaction.execute(
                    "UPDATE operation_journal SET phase = 'complete', state = 'committed', updated_at = ?1 \
                     WHERE journal_id = ?2 AND state = 'prepared'",
                    rusqlite::params![now, journal_id.to_string()],
                )?;
                transaction.commit()?;
                Ok::<_, DbError>(())
            })
            .await?;
        self.load_node(node_id).await
    }
}

fn snapshot_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StorageNodeSnapshot> {
    let size_bytes: Option<i64> = row.get(6)?;
    Ok(StorageNodeSnapshot {
        node_id: row.get(0)?,
        parent_node_id: row.get(1)?,
        node_type: row.get(2)?,
        display_name: row.get(3)?,
        state: row.get(4)?,
        system_role: row.get(5)?,
        size_bytes: size_bytes.and_then(|value| u64::try_from(value).ok()),
        mime_type: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn load_node_snapshot(
    connection: &rusqlite::Connection,
    node_id: String,
) -> std::result::Result<Option<StorageNodeSnapshot>, DbError> {
    connection
        .query_row(
            "SELECT n.node_id, n.parent_node_id, n.node_type, n.display_name, n.state, \
                    n.system_role, o.plaintext_size, o.detected_mime, n.updated_at \
             FROM storage_nodes n \
             LEFT JOIN file_versions v ON v.version_id = n.current_version_id \
             LEFT JOIN storage_objects o ON o.object_id = v.object_id \
             WHERE n.node_id = ?1 AND n.state != 'deleted'",
            [node_id],
            snapshot_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn validate_parent_directory(
    transaction: &rusqlite::Transaction<'_>,
    scope_id: StorageScopeId,
    parent_node_id: StorageNodeId,
    allow_trash: bool,
) -> std::result::Result<(), DbError> {
    let row: Option<(String, String, String, Option<String>)> = transaction
        .query_row(
            "SELECT n.scope_id, n.node_type, n.state, n.system_role \
             FROM storage_nodes n JOIN storage_scopes s ON s.scope_id = n.scope_id \
             WHERE n.node_id = ?1 AND s.scope_type = 'universal' AND s.state = 'active'",
            [parent_node_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let (stored_scope, node_type, state, system_role) =
        row.ok_or_else(|| invariant("parent directory was not found in Universal Storage"))?;
    if stored_scope != scope_id.to_string() || node_type != "directory" || state != "active" {
        return Err(invariant(
            "parent is not an active Universal Storage directory",
        ));
    }
    if !allow_trash && system_role.as_deref() == Some("trash") {
        return Err(invariant(
            "new content cannot be created directly inside Trash",
        ));
    }
    Ok(())
}

fn reject_sibling_conflict(
    transaction: &rusqlite::Transaction<'_>,
    scope_id: StorageScopeId,
    parent_node_id: StorageNodeId,
    normalized_name: &str,
    excluding: Option<StorageNodeId>,
) -> std::result::Result<(), DbError> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM storage_nodes \
         WHERE scope_id = ?1 AND parent_node_id = ?2 AND normalized_name = ?3 \
           AND state IN ('active', 'importing') AND (?4 IS NULL OR node_id != ?4))",
        rusqlite::params![
            scope_id.to_string(),
            parent_node_id.to_string(),
            normalized_name,
            excluding.map(|value| value.to_string()),
        ],
        |row| row.get::<_, i64>(0).map(|value| value != 0),
    )?;
    if exists {
        return Err(invariant("an active sibling already uses that name"));
    }
    Ok(())
}

fn validate_user_name(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_DIRECTORY_NAME_BYTES
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.chars().any(char::is_control)
    {
        return Err(MukeiError::Invariant("invalid storage node name".into()));
    }
    Ok(trimmed.to_owned())
}

fn parse_node_id(value: String) -> std::result::Result<StorageNodeId, DbError> {
    Uuid::parse_str(&value)
        .map(StorageNodeId)
        .map_err(|_| invariant("persisted storage node id is not a UUID"))
}

fn invariant(message: impl Into<String>) -> DbError {
    DbError::Domain(MukeiError::Invariant(message.into()))
}
