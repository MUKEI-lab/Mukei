//! Crash-safe Trash and recovery operations for isolated chat workspaces.
//!
//! Trashing is a logical, reversible move. File versions and immutable objects
//! are never deleted. The original parent is recorded in the operation journal
//! in the same IMMEDIATE transaction that moves the node into the workspace's
//! mandatory Trash directory.

use crate::error::{MukeiError, Result};
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use crate::storage::universal::{
    ChatId, StorageNodeId, StorageScopeId, WorkspaceAccessContext, WorkspaceId,
};
use rusqlite::{OptionalExtension, TransactionBehavior};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashReceipt {
    pub journal_id: Uuid,
    pub node_id: StorageNodeId,
    pub scope_id: StorageScopeId,
    pub original_parent_node_id: StorageNodeId,
    pub trash_node_id: StorageNodeId,
    pub trashed_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreReceipt {
    pub journal_id: Uuid,
    pub node_id: StorageNodeId,
    pub restored_parent_node_id: StorageNodeId,
    pub restored_at: String,
}

pub struct TrashRepository;

impl TrashRepository {
    /// Move an active, user-owned node into the workspace Trash atomically.
    /// System directories and workspace roots are never trashable.
    pub async fn trash_node(
        pool: &DatabasePool,
        access: WorkspaceAccessContext,
        node_id: StorageNodeId,
    ) -> Result<TrashReceipt> {
        pool.with_conn(move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

            let row: Option<(String, String, String, Option<String>, Option<String>, String)> =
                transaction
                    .query_row(
                        "SELECT n.scope_id, s.workspace_id, s.owner_chat_id, n.parent_node_id, n.system_role, n.state \
                         FROM storage_nodes n \
                         JOIN storage_scopes s ON s.scope_id = n.scope_id \
                         WHERE n.node_id = ?1 AND s.scope_type = 'workspace' AND s.state = 'active'",
                        [node_id.to_string()],
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                                row.get(5)?,
                            ))
                        },
                    )
                    .optional()?;

            let (scope_raw, workspace_raw, chat_raw, parent_raw, system_role, state) =
                row.ok_or_else(|| invariant("node was not found in an active workspace"))?;
            authorize(&access, &workspace_raw, &chat_raw)?;
            if state != "active" {
                return Err(invariant(format!("only active nodes can be trashed: {state}")));
            }
            if system_role.is_some() {
                return Err(invariant("system-owned workspace directories cannot be trashed"));
            }

            let scope_id = parse_scope_id(scope_raw)?;
            let original_parent_node_id = parent_raw
                .map(parse_node_id)
                .transpose()?
                .ok_or_else(|| invariant("workspace root cannot be trashed"))?;
            let trash_node_id = load_trash_node(&transaction, scope_id)?;
            let journal_id = Uuid::new_v4();
            let now = chrono::Utc::now().to_rfc3339();
            let payload = format!(
                "{{\"original_parent_node_id\":\"{}\",\"trash_node_id\":\"{}\"}}",
                original_parent_node_id, trash_node_id
            );

            transaction.execute(
                "INSERT INTO operation_journal \
                    (journal_id, operation_type, scope_id, node_id, transaction_id, phase, payload_json, state, created_at, updated_at) \
                 VALUES (?1, 'trash_node', ?2, ?3, NULL, 'database_move', ?4, 'prepared', ?5, ?5)",
                rusqlite::params![
                    journal_id.to_string(),
                    scope_id.to_string(),
                    node_id.to_string(),
                    &payload,
                    &now,
                ],
            )?;

            let updated = transaction.execute(
                "UPDATE storage_nodes \
                 SET parent_node_id = ?1, state = 'trashed', trashed_at = ?2, updated_at = ?2 \
                 WHERE node_id = ?3 AND scope_id = ?4 AND state = 'active' AND system_role IS NULL",
                rusqlite::params![
                    trash_node_id.to_string(),
                    &now,
                    node_id.to_string(),
                    scope_id.to_string(),
                ],
            )?;
            if updated != 1 {
                return Err(invariant("node changed while moving to Trash; transaction aborted"));
            }

            transaction.execute(
                "UPDATE operation_journal SET phase = 'complete', state = 'committed', updated_at = ?1 WHERE journal_id = ?2",
                rusqlite::params![&now, journal_id.to_string()],
            )?;
            transaction.commit()?;

            Ok::<_, DbError>(TrashReceipt {
                journal_id,
                node_id,
                scope_id,
                original_parent_node_id,
                trash_node_id,
                trashed_at: now,
            })
        })
        .await
    }

    /// Restore a trashed node to the parent recorded by its latest committed
    /// trash journal. Missing parents and sibling-name conflicts fail closed.
    pub async fn restore_node(
        pool: &DatabasePool,
        access: WorkspaceAccessContext,
        node_id: StorageNodeId,
    ) -> Result<RestoreReceipt> {
        pool.with_conn(move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

            let row: Option<(String, String, String, String)> = transaction
                .query_row(
                    "SELECT n.scope_id, s.workspace_id, s.owner_chat_id, n.state \
                     FROM storage_nodes n \
                     JOIN storage_scopes s ON s.scope_id = n.scope_id \
                     WHERE n.node_id = ?1 AND s.scope_type = 'workspace' AND s.state = 'active'",
                    [node_id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()?;
            let (scope_raw, workspace_raw, chat_raw, state) =
                row.ok_or_else(|| invariant("trashed node was not found in an active workspace"))?;
            authorize(&access, &workspace_raw, &chat_raw)?;
            if state != "trashed" {
                return Err(invariant(format!("only trashed nodes can be restored: {state}")));
            }
            let scope_id = parse_scope_id(scope_raw)?;

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
            let restored_parent_node_id = parent_raw
                .map(parse_node_id)
                .transpose()?
                .ok_or_else(|| invariant("committed trash journal is missing original parent"))?;

            let parent_active: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM storage_nodes WHERE node_id = ?1 AND scope_id = ?2 AND node_type = 'directory' AND state = 'active')",
                rusqlite::params![restored_parent_node_id.to_string(), scope_id.to_string()],
                |row| row.get::<_, i64>(0).map(|value| value != 0),
            )?;
            if !parent_active {
                return Err(invariant("original parent is missing or not active; restore aborted"));
            }

            let journal_id = Uuid::new_v4();
            let now = chrono::Utc::now().to_rfc3339();
            let payload = format!(
                "{{\"restored_parent_node_id\":\"{}\"}}",
                restored_parent_node_id
            );
            transaction.execute(
                "INSERT INTO operation_journal \
                    (journal_id, operation_type, scope_id, node_id, transaction_id, phase, payload_json, state, created_at, updated_at) \
                 VALUES (?1, 'restore_node', ?2, ?3, NULL, 'database_move', ?4, 'prepared', ?5, ?5)",
                rusqlite::params![
                    journal_id.to_string(),
                    scope_id.to_string(),
                    node_id.to_string(),
                    &payload,
                    &now,
                ],
            )?;

            let updated = transaction.execute(
                "UPDATE storage_nodes \
                 SET parent_node_id = ?1, state = 'active', trashed_at = NULL, updated_at = ?2 \
                 WHERE node_id = ?3 AND scope_id = ?4 AND state = 'trashed'",
                rusqlite::params![
                    restored_parent_node_id.to_string(),
                    &now,
                    node_id.to_string(),
                    scope_id.to_string(),
                ],
            )?;
            if updated != 1 {
                return Err(invariant("node changed while restoring; transaction aborted"));
            }
            transaction.execute(
                "UPDATE operation_journal SET phase = 'complete', state = 'committed', updated_at = ?1 WHERE journal_id = ?2",
                rusqlite::params![&now, journal_id.to_string()],
            )?;
            transaction.commit()?;

            Ok::<_, DbError>(RestoreReceipt {
                journal_id,
                node_id,
                restored_parent_node_id,
                restored_at: now,
            })
        })
        .await
    }
}

fn load_trash_node(
    transaction: &rusqlite::Transaction<'_>,
    scope_id: StorageScopeId,
) -> std::result::Result<StorageNodeId, DbError> {
    let raw: String = transaction.query_row(
        "SELECT node_id FROM storage_nodes WHERE scope_id = ?1 AND system_role = 'trash' AND state = 'active'",
        [scope_id.to_string()],
        |row| row.get(0),
    )?;
    parse_node_id(raw)
}

fn authorize(
    access: &WorkspaceAccessContext,
    workspace_raw: &str,
    chat_raw: &str,
) -> std::result::Result<(), DbError> {
    let workspace_id = WorkspaceId(
        Uuid::parse_str(workspace_raw)
            .map_err(|_| invariant("persisted workspace id is not a UUID"))?,
    );
    let chat_id = ChatId::parse(chat_raw.to_owned())
        .map_err(|error| invariant(format!("invalid persisted chat id: {error}")))?;
    access
        .authorize(&chat_id, workspace_id)
        .map_err(|error| invariant(error.to_string()))
}

fn parse_scope_id(value: String) -> std::result::Result<StorageScopeId, DbError> {
    Uuid::parse_str(&value)
        .map(StorageScopeId)
        .map_err(|_| invariant("persisted scope id is not a UUID"))
}

fn parse_node_id(value: String) -> std::result::Result<StorageNodeId, DbError> {
    Uuid::parse_str(&value)
        .map(StorageNodeId)
        .map_err(|_| invariant("persisted node id is not a UUID"))
}

fn invariant(message: impl Into<String>) -> DbError {
    DbError::Domain(MukeiError::Invariant(message.into()))
}
