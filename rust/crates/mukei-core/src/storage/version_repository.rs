//! Copy-on-write file version persistence for Universal Storage workspaces.
//!
//! Existing immutable objects and file-version rows are never mutated. A new
//! version is appended and the owning file node is advanced atomically inside
//! one IMMEDIATE transaction after workspace authorization succeeds.

use crate::error::{MukeiError, Result};
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use crate::storage::universal::{
    ChatId, FileVersionId, StorageNodeId, StorageObjectId, WorkspaceAccessContext, WorkspaceId,
};
use rusqlite::{OptionalExtension, TransactionBehavior};
use uuid::Uuid;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum VersionCreator {
    UserImport,
    UserEdit,
    AssistantGeneration,
    Research,
    SystemRecovery,
}

impl VersionCreator {
    const fn as_str(self) -> &'static str {
        match self {
            Self::UserImport => "user_import",
            Self::UserEdit => "user_edit",
            Self::AssistantGeneration => "assistant_generation",
            Self::Research => "research",
            Self::SystemRecovery => "system_recovery",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewFileVersion {
    pub object_id: StorageObjectId,
    pub created_by: VersionCreator,
    pub original_filename: Option<String>,
    pub detected_encoding: Option<String>,
    pub language_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedFileVersion {
    pub version_id: FileVersionId,
    pub node_id: StorageNodeId,
    pub object_id: StorageObjectId,
    pub previous_version_id: Option<FileVersionId>,
    pub version_number: u32,
    pub created_by: VersionCreator,
    pub created_at: String,
}

pub struct FileVersionRepository;

impl FileVersionRepository {
    /// Append a new immutable version and atomically move the file node's
    /// current-version pointer. The previous version remains reachable and is
    /// never overwritten or deleted.
    pub async fn append_copy_on_write(
        pool: &DatabasePool,
        access: WorkspaceAccessContext,
        node_id: StorageNodeId,
        new_version: NewFileVersion,
    ) -> Result<PersistedFileVersion> {
        pool.with_conn(move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

            let node: Option<(String, String, String, String, Option<String>)> = transaction
                .query_row(
                    "SELECT s.workspace_id, s.owner_chat_id, n.node_type, n.state, n.current_version_id \
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
                        ))
                    },
                )
                .optional()?;

            let (workspace_id, owner_chat_id, node_type, node_state, previous_version_raw) =
                node.ok_or_else(|| invariant("file node was not found in an active workspace"))?;

            let requested_workspace_id = parse_workspace_id(&workspace_id)?;
            let requested_chat_id = ChatId::parse(owner_chat_id)
                .map_err(|error| invariant(format!("invalid persisted chat id: {error}")))?;
            access
                .authorize(&requested_chat_id, requested_workspace_id)
                .map_err(|error| invariant(error.to_string()))?;

            if node_type != "file" {
                return Err(invariant("copy-on-write target is not a file node"));
            }
            if node_state != "active" {
                return Err(invariant(format!(
                    "copy-on-write target is not active: {node_state}"
                )));
            }

            let object_exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM storage_objects WHERE object_id = ?1 AND integrity_state = 'verified')",
                [new_version.object_id.to_string()],
                |row| row.get::<_, i64>(0).map(|value| value != 0),
            )?;
            if !object_exists {
                return Err(invariant(
                    "new version object is missing or has not passed integrity verification",
                ));
            }

            let previous_version_id = previous_version_raw
                .map(parse_version_id)
                .transpose()?;
            let version_number = if let Some(previous) = previous_version_id {
                let previous_number: i64 = transaction.query_row(
                    "SELECT version_number FROM file_versions WHERE version_id = ?1",
                    [previous.to_string()],
                    |row| row.get(0),
                )?;
                next_version_number(previous_number)?
            } else {
                1
            };

            let version_id = FileVersionId::new();
            let created_at = chrono::Utc::now().to_rfc3339();
            transaction.execute(
                "INSERT INTO file_versions \
                    (version_id, object_id, previous_version_id, version_number, created_by, \
                     original_filename, detected_encoding, language_id, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    version_id.to_string(),
                    new_version.object_id.to_string(),
                    previous_version_id.map(|value| value.to_string()),
                    i64::from(version_number),
                    new_version.created_by.as_str(),
                    new_version.original_filename,
                    new_version.detected_encoding,
                    new_version.language_id,
                    &created_at,
                ],
            )?;

            let updated = transaction.execute(
                "UPDATE storage_nodes \
                 SET current_version_id = ?1, updated_at = ?2 \
                 WHERE node_id = ?3 AND state = 'active' AND current_version_id IS ?4",
                rusqlite::params![
                    version_id.to_string(),
                    &created_at,
                    node_id.to_string(),
                    previous_version_id.map(|value| value.to_string()),
                ],
            )?;
            if updated != 1 {
                return Err(invariant(
                    "file node changed while appending a version; transaction aborted",
                ));
            }

            transaction.commit()?;
            Ok::<_, DbError>(PersistedFileVersion {
                version_id,
                node_id,
                object_id: new_version.object_id,
                previous_version_id,
                version_number,
                created_by: new_version.created_by,
                created_at,
            })
        })
        .await
    }

    pub async fn history(
        pool: &DatabasePool,
        access: WorkspaceAccessContext,
        node_id: StorageNodeId,
    ) -> Result<Vec<PersistedFileVersion>> {
        pool.with_conn(move |connection| {
            let ownership: Option<(String, String)> = connection
                .query_row(
                    "SELECT s.workspace_id, s.owner_chat_id \
                     FROM storage_nodes n JOIN storage_scopes s ON s.scope_id = n.scope_id \
                     WHERE n.node_id = ?1 AND s.scope_type = 'workspace' AND n.node_type = 'file'",
                    [node_id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let (workspace_id, chat_id) =
                ownership.ok_or_else(|| invariant("file node was not found in a workspace"))?;
            let workspace_id = parse_workspace_id(&workspace_id)?;
            let chat_id = ChatId::parse(chat_id)
                .map_err(|error| invariant(format!("invalid persisted chat id: {error}")))?;
            access
                .authorize(&chat_id, workspace_id)
                .map_err(|error| invariant(error.to_string()))?;

            let mut statement = connection.prepare(
                "WITH RECURSIVE version_chain(version_id) AS ( \
                    SELECT current_version_id FROM storage_nodes WHERE node_id = ?1 \
                    UNION ALL \
                    SELECT fv.previous_version_id FROM file_versions fv \
                    JOIN version_chain vc ON fv.version_id = vc.version_id \
                    WHERE fv.previous_version_id IS NOT NULL \
                 ) \
                 SELECT fv.version_id, fv.object_id, fv.previous_version_id, fv.version_number, \
                        fv.created_by, fv.created_at \
                 FROM file_versions fv JOIN version_chain vc ON vc.version_id = fv.version_id \
                 ORDER BY fv.version_number DESC",
            )?;
            let rows = statement.query_map([node_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?;

            let mut versions = Vec::new();
            for row in rows {
                let (version_id, object_id, previous, number, creator, created_at) = row?;
                versions.push(PersistedFileVersion {
                    version_id: parse_version_id(version_id)?,
                    node_id,
                    object_id: parse_object_id(object_id)?,
                    previous_version_id: previous.map(parse_version_id).transpose()?,
                    version_number: u32::try_from(number)
                        .map_err(|_| invariant("persisted version number is invalid"))?,
                    created_by: parse_creator(&creator)?,
                    created_at,
                });
            }
            Ok::<_, DbError>(versions)
        })
        .await
    }
}

fn next_version_number(previous: i64) -> std::result::Result<u32, DbError> {
    let previous =
        u32::try_from(previous).map_err(|_| invariant("persisted version number is invalid"))?;
    previous
        .checked_add(1)
        .ok_or_else(|| invariant("file version number overflow"))
}

fn parse_creator(value: &str) -> std::result::Result<VersionCreator, DbError> {
    match value {
        "user_import" => Ok(VersionCreator::UserImport),
        "user_edit" => Ok(VersionCreator::UserEdit),
        "assistant_generation" => Ok(VersionCreator::AssistantGeneration),
        "research" => Ok(VersionCreator::Research),
        "system_recovery" => Ok(VersionCreator::SystemRecovery),
        _ => Err(invariant(format!("unknown version creator: {value}"))),
    }
}

fn parse_workspace_id(value: &str) -> std::result::Result<WorkspaceId, DbError> {
    Uuid::parse_str(value)
        .map(WorkspaceId)
        .map_err(|_| invariant("persisted workspace id is invalid"))
}

fn parse_version_id(value: String) -> std::result::Result<FileVersionId, DbError> {
    Uuid::parse_str(&value)
        .map(FileVersionId)
        .map_err(|_| invariant("persisted file version id is invalid"))
}

fn parse_object_id(value: String) -> std::result::Result<StorageObjectId, DbError> {
    Uuid::parse_str(&value)
        .map(StorageObjectId)
        .map_err(|_| invariant("persisted storage object id is invalid"))
}

fn invariant(message: impl Into<String>) -> DbError {
    DbError::Domain(MukeiError::Invariant(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creator_values_match_the_append_only_schema() {
        assert_eq!(VersionCreator::UserImport.as_str(), "user_import");
        assert_eq!(VersionCreator::UserEdit.as_str(), "user_edit");
        assert_eq!(
            VersionCreator::AssistantGeneration.as_str(),
            "assistant_generation"
        );
        assert_eq!(VersionCreator::Research.as_str(), "research");
        assert_eq!(VersionCreator::SystemRecovery.as_str(), "system_recovery");
    }

    #[test]
    fn version_numbers_increment_without_wrapping() {
        assert_eq!(next_version_number(1).unwrap(), 2);
        assert!(next_version_number(i64::from(u32::MAX)).is_err());
        assert!(next_version_number(-1).is_err());
    }
}
