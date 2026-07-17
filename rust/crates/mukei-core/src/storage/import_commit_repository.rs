//! Atomic publication of encrypted imported files into Universal Storage or a chat workspace.
//!
//! The object-store publication happens before this repository is called. This layer records a
//! durable `applied_filesystem` journal entry, then commits the verified object metadata, initial
//! immutable version, logical file node, and import-state transition in one IMMEDIATE transaction.
//! A retry after process death is idempotent and returns the already-committed node.

use crate::error::{MukeiError, Result};
use crate::storage::file_policy::{AllowedFileName, FileAdmissionRule, MAX_FILENAME_BYTES};
use crate::storage::object_store::StoredObject;
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use crate::storage::universal::{
    ChatId, DuplicatePolicy, FileVersionId, ImportTransactionId, StorageNodeId, StorageObjectId,
    StorageScopeId, WorkspaceAccessContext, WorkspaceId,
};
use rusqlite::{OptionalExtension, TransactionBehavior};
use serde_json::json;
use uuid::Uuid;

const OPERATION_TYPE: &str = "file_import_commit";
const MAX_CONFLICT_ATTEMPTS: u32 = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportAuthorization {
    Universal,
    Workspace(WorkspaceAccessContext),
}

#[derive(Clone, Debug)]
pub struct ImportCommitRequest {
    pub transaction_id: ImportTransactionId,
    pub authorization: ImportAuthorization,
    pub admitted_name: AllowedFileName,
    pub stored_object: StoredObject,
    pub detected_format: String,
    pub detected_mime: Option<String>,
    pub detected_encoding: Option<String>,
    pub language_id: Option<String>,
    pub encryption_version: u32,
    pub duplicate_policy: DuplicatePolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportCommitReceipt {
    pub journal_id: String,
    pub scope_id: StorageScopeId,
    pub parent_node_id: StorageNodeId,
    pub node_id: StorageNodeId,
    pub version_id: FileVersionId,
    pub object_id: StorageObjectId,
    pub display_name: String,
    pub reused_object: bool,
    pub reused_version: bool,
}

#[derive(Clone, Debug)]
struct PreparedImport {
    journal_id: String,
    scope_id: StorageScopeId,
    parent_node_id: StorageNodeId,
}

pub struct ImportCommitRepository;

impl ImportCommitRepository {
    /// Publish one already-encrypted object as a logical file entry.
    ///
    /// `ReplaceWithNewVersion` is deliberately rejected here. Replacing an existing logical file
    /// is an explicit copy-on-write operation handled by `FileVersionRepository`; an import must
    /// never silently advance a sibling file's version pointer.
    pub async fn commit(
        pool: &DatabasePool,
        request: ImportCommitRequest,
    ) -> Result<ImportCommitReceipt> {
        validate_request(&request)?;

        if let Some(receipt) = load_committed_receipt(pool, request.transaction_id).await? {
            return Ok(receipt);
        }

        let prepared = prepare_filesystem_journal(pool, request.clone()).await?;
        publish_database_state(pool, request, prepared).await
    }
}

async fn load_committed_receipt(
    pool: &DatabasePool,
    transaction_id: ImportTransactionId,
) -> Result<Option<ImportCommitReceipt>> {
    pool.with_conn(move |connection| {
        let row: Option<(String, String, String, String, String, String, String)> = connection
            .query_row(
                "SELECT j.journal_id, j.scope_id, n.parent_node_id, n.node_id, \
                        n.current_version_id, fv.object_id, n.display_name \
                 FROM operation_journal j \
                 JOIN storage_nodes n ON n.node_id = j.node_id \
                 JOIN file_versions fv ON fv.version_id = n.current_version_id \
                 WHERE j.transaction_id = ?1 AND j.operation_type = ?2 \
                   AND j.state = 'committed' \
                 ORDER BY j.created_at DESC LIMIT 1",
                rusqlite::params![transaction_id.to_string(), OPERATION_TYPE],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .optional()?;

        row.map(|row| {
            Ok(ImportCommitReceipt {
                journal_id: row.0,
                scope_id: parse_scope_id(&row.1)?,
                parent_node_id: parse_node_id(&row.2)?,
                node_id: parse_node_id(&row.3)?,
                version_id: parse_version_id(&row.4)?,
                object_id: parse_object_id(&row.5)?,
                display_name: row.6,
                reused_object: true,
                reused_version: true,
            })
        })
        .transpose()
    })
    .await
}

async fn prepare_filesystem_journal(
    pool: &DatabasePool,
    request: ImportCommitRequest,
) -> Result<PreparedImport> {
    pool.with_conn(move |connection| {
        let transaction =
            connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (scope_id, parent_node_id) = validate_import_target(&transaction, &request)?;

        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT journal_id, state FROM operation_journal \
                 WHERE transaction_id = ?1 AND operation_type = ?2 \
                 ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![request.transaction_id.to_string(), OPERATION_TYPE],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        let journal_id = match existing {
            Some((journal_id, state)) => {
                if state == "committed" {
                    return Err(invariant(
                        "committed import journal exists without a readable file receipt",
                    ));
                }
                if !matches!(
                    state.as_str(),
                    "prepared" | "applied_filesystem" | "recovery_required"
                ) {
                    return Err(invariant(format!(
                        "import journal is in an unsupported recovery state: {state}"
                    )));
                }
                journal_id
            }
            None => {
                let journal_id = Uuid::new_v4().to_string();
                let relative_path = request
                    .stored_object
                    .relative_path
                    .to_str()
                    .ok_or_else(|| invariant("object relative path is not UTF-8"))?;
                let payload = json!({
                    "requested_name": request.admitted_name.display_name,
                    "candidate_object_id": request.stored_object.object_id.to_string(),
                    "plaintext_sha256": hex_digest(&request.stored_object.plaintext_sha256),
                    "plaintext_size": request.stored_object.plaintext_size,
                    "encrypted_size": request.stored_object.encrypted_size,
                    "relative_path": relative_path,
                    "encryption_version": request.encryption_version,
                })
                .to_string();
                let now = chrono::Utc::now().to_rfc3339();
                transaction.execute(
                    "INSERT INTO operation_journal \
                        (journal_id, operation_type, scope_id, node_id, transaction_id, phase, \
                         payload_json, state, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, NULL, ?4, 'object_published', ?5, \
                             'applied_filesystem', ?6, ?6)",
                    rusqlite::params![
                        &journal_id,
                        OPERATION_TYPE,
                        scope_id.to_string(),
                        request.transaction_id.to_string(),
                        payload,
                        now,
                    ],
                )?;
                journal_id
            }
        };

        transaction.commit()?;
        Ok::<_, DbError>(PreparedImport {
            journal_id,
            scope_id,
            parent_node_id,
        })
    })
    .await
}

async fn publish_database_state(
    pool: &DatabasePool,
    request: ImportCommitRequest,
    prepared: PreparedImport,
) -> Result<ImportCommitReceipt> {
    pool.with_conn(move |connection| {
        let transaction =
            connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (scope_id, parent_node_id) = validate_import_target(&transaction, &request)?;
        if scope_id != prepared.scope_id || parent_node_id != prepared.parent_node_id {
            return Err(invariant(
                "import target changed after filesystem publication; transaction aborted",
            ));
        }

        let (object_id, reused_object) = persist_or_reuse_object(&transaction, &request)?;
        let (version_id, reused_version) = persist_or_reuse_initial_version(
            &transaction,
            object_id,
            &request,
        )?;
        let display_name = resolve_display_name(
            &transaction,
            scope_id,
            parent_node_id,
            &request.admitted_name,
            request.duplicate_policy,
        )?;
        let normalized_name = display_name.to_ascii_lowercase();
        let node_id = StorageNodeId::new();
        let now = chrono::Utc::now().to_rfc3339();

        transaction.execute(
            "INSERT INTO storage_nodes \
                (node_id, scope_id, parent_node_id, node_type, display_name, normalized_name, \
                 current_version_id, system_role, state, created_at, updated_at, trashed_at) \
             VALUES (?1, ?2, ?3, 'file', ?4, ?5, ?6, NULL, 'active', ?7, ?7, NULL)",
            rusqlite::params![
                node_id.to_string(),
                scope_id.to_string(),
                parent_node_id.to_string(),
                &display_name,
                normalized_name,
                version_id.to_string(),
                &now,
            ],
        )?;

        let detected_extension = match request.admitted_name.rule {
            FileAdmissionRule::Extension(value) => Some(value),
            FileAdmissionRule::ExactName(_) => None,
        };
        let updated = transaction.execute(
            "UPDATE import_transactions \
             SET detected_extension = ?2, detected_mime = ?3, detected_encoding = ?4, \
                 state = 'indexing', updated_at = ?5 \
             WHERE transaction_id = ?1 AND state IN ('committing_node', 'recovering')",
            rusqlite::params![
                request.transaction_id.to_string(),
                detected_extension,
                request.detected_mime,
                request.detected_encoding,
                &now,
            ],
        )?;
        if updated != 1 {
            return Err(invariant(
                "import state changed while publishing the logical file node",
            ));
        }

        let journal_updated = transaction.execute(
            "UPDATE operation_journal \
             SET node_id = ?2, phase = 'database_committed', state = 'committed', updated_at = ?3 \
             WHERE journal_id = ?1 AND state IN ('prepared', 'applied_filesystem', 'recovery_required')",
            rusqlite::params![&prepared.journal_id, node_id.to_string(), &now],
        )?;
        if journal_updated != 1 {
            return Err(invariant(
                "import journal changed while publishing the logical file node",
            ));
        }

        transaction.commit()?;
        Ok::<_, DbError>(ImportCommitReceipt {
            journal_id: prepared.journal_id,
            scope_id,
            parent_node_id,
            node_id,
            version_id,
            object_id,
            display_name,
            reused_object,
            reused_version,
        })
    })
    .await
}

fn validate_request(request: &ImportCommitRequest) -> Result<()> {
    if request.detected_format.trim().is_empty() {
        return Err(MukeiError::Invariant(
            "detected file format must not be empty".into(),
        ));
    }
    if request.encryption_version == 0 {
        return Err(MukeiError::Invariant(
            "object encryption version must be non-zero".into(),
        ));
    }
    if request.duplicate_policy == DuplicatePolicy::ReplaceWithNewVersion {
        return Err(MukeiError::Invariant(
            "import replacement requires explicit copy-on-write versioning".into(),
        ));
    }
    Ok(())
}

fn validate_import_target(
    transaction: &rusqlite::Transaction<'_>,
    request: &ImportCommitRequest,
) -> std::result::Result<(StorageScopeId, StorageNodeId), DbError> {
    type TargetRow = (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
        String,
    );

    let row: Option<TargetRow> = transaction
        .query_row(
            "SELECT it.target_scope_id, it.target_parent_node_id, it.original_filename, it.state, \
                    s.scope_type, s.workspace_id, s.owner_chat_id, s.state, \
                    n.node_type, n.state, n.scope_id \
             FROM import_transactions it \
             JOIN storage_scopes s ON s.scope_id = it.target_scope_id \
             JOIN storage_nodes n ON n.node_id = it.target_parent_node_id \
             WHERE it.transaction_id = ?1",
            [request.transaction_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )
        .optional()?;
    let row = row.ok_or_else(|| invariant("import transaction target was not found"))?;

    if !matches!(row.3.as_str(), "committing_node" | "recovering") {
        return Err(invariant(format!(
            "import is not ready for logical publication: {}",
            row.3
        )));
    }
    if row.2.trim() != request.admitted_name.display_name {
        return Err(invariant(
            "admitted filename does not match the import transaction",
        ));
    }
    if row.7 != "active" || row.8 != "directory" || row.9 != "active" {
        return Err(invariant(
            "import target scope or parent directory is not active",
        ));
    }
    if row.0 != row.10 {
        return Err(invariant(
            "import parent directory belongs to a different storage scope",
        ));
    }

    match &request.authorization {
        ImportAuthorization::Universal => {
            if row.4 != "universal" || row.5.is_some() || row.6.is_some() {
                return Err(invariant("universal import targeted a chat workspace"));
            }
        }
        ImportAuthorization::Workspace(access) => {
            if row.4 != "workspace" {
                return Err(invariant("workspace import targeted Universal Storage"));
            }
            let workspace_id = row
                .5
                .as_deref()
                .ok_or_else(|| invariant("workspace scope is missing its workspace id"))?;
            let chat_id = row
                .6
                .as_deref()
                .ok_or_else(|| invariant("workspace scope is missing its owner chat id"))?;
            let workspace_id = parse_workspace_id(workspace_id)?;
            let chat_id = ChatId::parse(chat_id)
                .map_err(|error| invariant(format!("invalid persisted chat id: {error}")))?;
            access
                .authorize(&chat_id, workspace_id)
                .map_err(|error| invariant(error.to_string()))?;
        }
    }

    Ok((parse_scope_id(&row.0)?, parse_node_id(&row.1)?))
}

fn persist_or_reuse_object(
    transaction: &rusqlite::Transaction<'_>,
    request: &ImportCommitRequest,
) -> std::result::Result<(StorageObjectId, bool), DbError> {
    let relative_path = request
        .stored_object
        .relative_path
        .to_str()
        .ok_or_else(|| invariant("object relative path is not UTF-8"))?;
    let plaintext_size = i64::try_from(request.stored_object.plaintext_size)
        .map_err(|_| invariant("plaintext size exceeds SQLite integer range"))?;
    let encrypted_size = i64::try_from(request.stored_object.encrypted_size)
        .map_err(|_| invariant("encrypted size exceeds SQLite integer range"))?;

    let existing: Option<(String, String, i64, String)> = transaction
        .query_row(
            "SELECT object_id, relative_path, encrypted_size, integrity_state \
             FROM storage_objects WHERE plaintext_sha256 = ?1 AND plaintext_size = ?2",
            rusqlite::params![
                request.stored_object.plaintext_sha256.as_slice(),
                plaintext_size,
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    if let Some((object_id, persisted_path, persisted_encrypted_size, integrity_state)) = existing {
        if integrity_state != "verified"
            || persisted_path != relative_path
            || persisted_encrypted_size != encrypted_size
        {
            return Err(invariant(
                "deduplicated object metadata is inconsistent or unverified",
            ));
        }
        return Ok((parse_object_id(&object_id)?, true));
    }

    let object_id = request.stored_object.object_id;
    let now = chrono::Utc::now().to_rfc3339();
    transaction.execute(
        "INSERT INTO storage_objects \
            (object_id, plaintext_sha256, plaintext_size, encrypted_size, relative_path, \
             detected_format, detected_mime, encryption_version, integrity_state, created_at, verified_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'verified', ?9, ?9)",
        rusqlite::params![
            object_id.to_string(),
            request.stored_object.plaintext_sha256.as_slice(),
            plaintext_size,
            encrypted_size,
            relative_path,
            request.detected_format.trim(),
            request.detected_mime,
            i64::from(request.encryption_version),
            &now,
        ],
    )?;
    Ok((object_id, false))
}

fn persist_or_reuse_initial_version(
    transaction: &rusqlite::Transaction<'_>,
    object_id: StorageObjectId,
    request: &ImportCommitRequest,
) -> std::result::Result<(FileVersionId, bool), DbError> {
    let existing: Option<String> = transaction
        .query_row(
            "SELECT version_id FROM file_versions \
             WHERE object_id = ?1 AND version_number = 1 \
             ORDER BY created_at ASC LIMIT 1",
            [object_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(version_id) = existing {
        return Ok((parse_version_id(&version_id)?, true));
    }

    let version_id = FileVersionId::new();
    transaction.execute(
        "INSERT INTO file_versions \
            (version_id, object_id, previous_version_id, version_number, created_by, \
             original_filename, detected_encoding, language_id, created_at) \
         VALUES (?1, ?2, NULL, 1, 'user_import', ?3, ?4, ?5, ?6)",
        rusqlite::params![
            version_id.to_string(),
            object_id.to_string(),
            request.admitted_name.display_name,
            request.detected_encoding,
            request.language_id,
            chrono::Utc::now().to_rfc3339(),
        ],
    )?;
    Ok((version_id, false))
}

fn resolve_display_name(
    transaction: &rusqlite::Transaction<'_>,
    scope_id: StorageScopeId,
    parent_node_id: StorageNodeId,
    admitted: &AllowedFileName,
    policy: DuplicatePolicy,
) -> std::result::Result<String, DbError> {
    if !name_exists(
        transaction,
        scope_id,
        parent_node_id,
        &admitted.normalized_name,
    )? {
        return Ok(admitted.display_name.clone());
    }

    match policy {
        DuplicatePolicy::RejectNameConflict => {
            Err(invariant("an active sibling already uses this filename"))
        }
        DuplicatePolicy::RenameNewEntry => {
            for index in 2..=MAX_CONFLICT_ATTEMPTS {
                let candidate = conflict_display_name(&admitted.display_name, index);
                if !name_exists(
                    transaction,
                    scope_id,
                    parent_node_id,
                    &candidate.to_ascii_lowercase(),
                )? {
                    return Ok(candidate);
                }
            }
            Err(invariant(
                "unable to allocate a unique filename within the bounded conflict limit",
            ))
        }
        DuplicatePolicy::ReplaceWithNewVersion => Err(invariant(
            "import replacement requires explicit copy-on-write versioning",
        )),
    }
}

fn name_exists(
    transaction: &rusqlite::Transaction<'_>,
    scope_id: StorageScopeId,
    parent_node_id: StorageNodeId,
    normalized_name: &str,
) -> std::result::Result<bool, DbError> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM storage_nodes \
             WHERE scope_id = ?1 AND parent_node_id = ?2 AND normalized_name = ?3 \
               AND state IN ('active', 'importing'))",
            rusqlite::params![
                scope_id.to_string(),
                parent_node_id.to_string(),
                normalized_name,
            ],
            |row| row.get::<_, i64>(0).map(|value| value != 0),
        )
        .map_err(DbError::from)
}

fn conflict_display_name(original: &str, index: u32) -> String {
    let suffix = format!(" ({index})");
    let extension_split = original
        .rfind('.')
        .filter(|position| *position > 0 && *position + 1 < original.len());

    let (stem, extension) = match extension_split {
        Some(position) => (&original[..position], Some(&original[position..])),
        None => (original, None),
    };
    let extension = extension.unwrap_or("");
    let reserved = suffix.len() + extension.len();
    let stem_budget = MAX_FILENAME_BYTES.saturating_sub(reserved);
    let stem = truncate_utf8(stem, stem_budget);
    format!("{stem}{suffix}{extension}")
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> &str {
    if value.len() <= maximum_bytes {
        return value;
    }
    let mut boundary = maximum_bytes.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &value[..boundary]
}

fn parse_scope_id(value: &str) -> std::result::Result<StorageScopeId, DbError> {
    Uuid::parse_str(value)
        .map(StorageScopeId)
        .map_err(|_| invariant("persisted storage scope id is invalid"))
}

fn parse_node_id(value: &str) -> std::result::Result<StorageNodeId, DbError> {
    Uuid::parse_str(value)
        .map(StorageNodeId)
        .map_err(|_| invariant("persisted storage node id is invalid"))
}

fn parse_workspace_id(value: &str) -> std::result::Result<WorkspaceId, DbError> {
    Uuid::parse_str(value)
        .map(WorkspaceId)
        .map_err(|_| invariant("persisted workspace id is invalid"))
}

fn parse_version_id(value: &str) -> std::result::Result<FileVersionId, DbError> {
    Uuid::parse_str(value)
        .map(FileVersionId)
        .map_err(|_| invariant("persisted file version id is invalid"))
}

fn parse_object_id(value: &str) -> std::result::Result<StorageObjectId, DbError> {
    Uuid::parse_str(value)
        .map(StorageObjectId)
        .map_err(|_| invariant("persisted storage object id is invalid"))
}

fn hex_digest(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn invariant(message: impl Into<String>) -> DbError {
    DbError::Domain(MukeiError::Invariant(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_suffix_preserves_the_last_extension() {
        assert_eq!(conflict_display_name("notes.md", 2), "notes (2).md");
        assert_eq!(
            conflict_display_name("archive.data.json", 3),
            "archive.data (3).json"
        );
    }

    #[test]
    fn exact_dot_names_are_treated_as_extensionless() {
        assert_eq!(conflict_display_name(".env", 2), ".env (2)");
        assert_eq!(conflict_display_name("README", 2), "README (2)");
    }

    #[test]
    fn conflict_names_remain_within_the_filename_byte_limit() {
        let original = format!("{}.md", "न".repeat(120));
        let candidate = conflict_display_name(&original, 10_000);
        assert!(candidate.len() <= MAX_FILENAME_BYTES);
        assert!(candidate.ends_with(" (10000).md"));
        assert!(candidate.is_char_boundary(candidate.len()));
    }
}
