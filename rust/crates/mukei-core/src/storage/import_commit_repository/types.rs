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
