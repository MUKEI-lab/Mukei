//! Bounded import of Android-staged files into encrypted chat workspaces.
//!
//! Android owns `content://` access and copies the selected document into an
//! app-private staging jail. This service validates that staged file, records
//! every durable phase, encrypts it into the immutable object store, and then
//! publishes a logical file under the chat workspace's `Uploaded files/` node.

use crate::error::MukeiError;
use crate::storage::file_policy::{admit_file_name, FileAdmissionError, FileAdmissionRule};
use crate::storage::import_commit_repository::{
    ImportAuthorization, ImportCommitRepository, ImportCommitRequest,
};
use crate::storage::import_journal::{ImportJournalRepository, ImportState};
use crate::storage::object_store::{ImmutableObjectStore, ObjectCipher, ObjectStoreError};
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use crate::storage::universal::{
    ChatId, DuplicatePolicy, ImportTransactionId, StorageNodeId, StorageObjectId, StorageScopeId,
    WorkspaceAccessContext,
};
use crate::storage::universal_repository::UniversalStorageRepository;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Defensive default for Phase-1 text/source imports.
pub const DEFAULT_MAX_STAGED_IMPORT_BYTES: u64 = 32 * 1024 * 1024;

/// One Android-staged file destined for a chat's `Uploaded files/` directory.
#[derive(Clone, Debug)]
pub struct WorkspaceStagedImportRequest {
    pub chat_id: ChatId,
    pub staged_path: PathBuf,
    pub original_filename: String,
    pub detected_mime: Option<String>,
    pub expected_size: Option<u64>,
    pub duplicate_policy: DuplicatePolicy,
    pub source_uri_fingerprint: Option<String>,
}

/// One Android-staged file destined for an explicit Universal Storage directory.
#[derive(Clone, Debug)]
pub struct UniversalStagedImportRequest {
    pub parent_node_id: StorageNodeId,
    pub staged_path: PathBuf,
    pub original_filename: String,
    pub detected_mime: Option<String>,
    pub expected_size: Option<u64>,
    pub duplicate_policy: DuplicatePolicy,
    pub source_uri_fingerprint: Option<String>,
}

/// Durable result returned only after the encrypted object and logical node commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceStagedImportReceipt {
    pub transaction_id: ImportTransactionId,
    pub node_id: StorageNodeId,
    pub object_id: StorageObjectId,
    pub display_name: String,
    pub plaintext_size: u64,
    pub deduplicated: bool,
    pub staged_file_removed: bool,
}

/// Stable failures surfaced to runtime operation events without leaking paths.
#[derive(Debug, thiserror::Error)]
pub enum StagedImportError {
    #[error(transparent)]
    FileAdmission(#[from] FileAdmissionError),
    #[error("staged file escaped the app-private staging jail")]
    UnsafeStagingPath,
    #[error("staged file does not exist or is not a regular file")]
    MissingStagedFile,
    #[error("staged file exceeds the import size limit")]
    FileTooLarge,
    #[error("staged file size differs from the platform response")]
    SizeMismatch,
    #[error("staged file changed while it was being read")]
    StagedFileChanged,
    #[error("staged file is not valid text encoded as UTF-8")]
    NonUtf8Text,
    #[error("staged import was cancelled before object publication")]
    Cancelled,
    #[error("staged import service configuration is invalid")]
    InvalidConfiguration,
    #[error("blocking staged-file task failed: {0}")]
    BlockingTask(String),
    #[error(transparent)]
    ObjectStore(#[from] ObjectStoreError),
    #[error(transparent)]
    Storage(#[from] MukeiError),
}

impl StagedImportError {
    /// Stable machine code suitable for operation projections and telemetry-free UI.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::FileAdmission(_) => "file_policy_rejected",
            Self::UnsafeStagingPath => "unsafe_staging_path",
            Self::MissingStagedFile => "staged_file_missing",
            Self::FileTooLarge => "staged_file_too_large",
            Self::SizeMismatch => "staged_file_size_mismatch",
            Self::StagedFileChanged => "staged_file_changed",
            Self::NonUtf8Text => "non_utf8_text_rejected",
            Self::Cancelled => "storage_import_cancelled",
            Self::InvalidConfiguration => "storage_import_configuration_invalid",
            Self::BlockingTask(_) => "storage_import_blocking_task_failed",
            Self::ObjectStore(_) => "encrypted_object_store_failed",
            Self::Storage(_) => "storage_import_commit_failed",
        }
    }
}

/// Runtime-facing import boundary. Implementations must preserve chat isolation.
#[async_trait::async_trait]
pub trait StagedFileImporter: Send + Sync {
    async fn import_workspace_file(
        &self,
        request: WorkspaceStagedImportRequest,
        cancellation: CancellationToken,
    ) -> Result<WorkspaceStagedImportReceipt, StagedImportError>;

    async fn import_universal_file(
        &self,
        _request: UniversalStagedImportRequest,
        _cancellation: CancellationToken,
    ) -> Result<WorkspaceStagedImportReceipt, StagedImportError> {
        Err(StagedImportError::InvalidConfiguration)
    }
}

/// Filesystem/database importer parameterized by an authenticated object cipher.
pub struct WorkspaceStagedImportService<C> {
    pool: Arc<DatabasePool>,
    object_store: Arc<ImmutableObjectStore<C>>,
    staging_root: PathBuf,
    max_import_bytes: u64,
}

impl<C: ObjectCipher> WorkspaceStagedImportService<C> {
    pub fn new(
        pool: Arc<DatabasePool>,
        object_store: Arc<ImmutableObjectStore<C>>,
        staging_root: impl Into<PathBuf>,
        max_import_bytes: u64,
    ) -> Result<Self, StagedImportError> {
        if max_import_bytes == 0 || object_store.encryption_version() == 0 {
            return Err(StagedImportError::InvalidConfiguration);
        }
        let staging_root = staging_root.into();
        fs::create_dir_all(&staging_root).map_err(ObjectStoreError::Io)?;
        let staging_root = fs::canonicalize(staging_root).map_err(ObjectStoreError::Io)?;
        Ok(Self {
            pool,
            object_store,
            staging_root,
            max_import_bytes,
        })
    }

    async fn execute_import(
        &self,
        transaction_id: ImportTransactionId,
        request: WorkspaceStagedImportRequest,
        admitted_name: crate::storage::file_policy::AllowedFileName,
        canonical_path: PathBuf,
        cancellation: CancellationToken,
    ) -> Result<WorkspaceStagedImportReceipt, StagedImportError>
    where
        C: Send + Sync + 'static,
    {
        transition(&self.pool, transaction_id, ImportState::Validating).await?;
        cancel_if_requested(&self.pool, transaction_id, &cancellation).await?;

        transition(&self.pool, transaction_id, ImportState::Copying).await?;
        let path_for_read = canonical_path.clone();
        let maximum = self.max_import_bytes;
        let expected_size = request.expected_size;
        let bytes = tokio::task::spawn_blocking(move || {
            read_bounded_staged_file(&path_for_read, maximum, expected_size)
        })
        .await
        .map_err(|error| StagedImportError::BlockingTask(error.to_string()))??;
        ImportJournalRepository::record_progress(&self.pool, transaction_id, bytes.len() as u64)
            .await?;
        cancel_if_requested(&self.pool, transaction_id, &cancellation).await?;

        transition(&self.pool, transaction_id, ImportState::Hashing).await?;
        validate_text_content(&bytes)?;
        cancel_if_requested(&self.pool, transaction_id, &cancellation).await?;

        transition(&self.pool, transaction_id, ImportState::Encrypting).await?;
        cancel_if_requested(&self.pool, transaction_id, &cancellation).await?;
        let object_store = Arc::clone(&self.object_store);
        let stored_object = tokio::task::spawn_blocking(move || object_store.put(&bytes))
            .await
            .map_err(|error| StagedImportError::BlockingTask(error.to_string()))??;

        transition(&self.pool, transaction_id, ImportState::CommittingObject).await?;
        transition(&self.pool, transaction_id, ImportState::CommittingNode).await?;
        let plaintext_size = stored_object.plaintext_size;
        let deduplicated = stored_object.deduplicated;
        let detected_format = match &admitted_name.rule {
            FileAdmissionRule::Extension(extension) => extension.to_string(),
            FileAdmissionRule::ExactName(name) => format!("exact:{name}"),
        };
        let workspace =
            UniversalStorageRepository::ensure_workspace(&self.pool, request.chat_id.clone())
                .await?;
        let receipt = ImportCommitRepository::commit(
            &self.pool,
            ImportCommitRequest {
                transaction_id,
                authorization: ImportAuthorization::Workspace(WorkspaceAccessContext {
                    chat_id: request.chat_id,
                    workspace_id: workspace.workspace_id,
                }),
                admitted_name,
                stored_object,
                detected_format,
                detected_mime: request
                    .detected_mime
                    .filter(|value| !value.trim().is_empty()),
                detected_encoding: Some("utf-8".to_string()),
                language_id: None,
                encryption_version: self.object_store.encryption_version(),
                duplicate_policy: request.duplicate_policy,
            },
        )
        .await?;
        transition(&self.pool, transaction_id, ImportState::Completed).await?;

        let cleanup_path = canonical_path;
        let staged_file_removed =
            tokio::task::spawn_blocking(move || fs::remove_file(cleanup_path).is_ok())
                .await
                .unwrap_or(false);
        Ok(WorkspaceStagedImportReceipt {
            transaction_id,
            node_id: receipt.node_id,
            object_id: receipt.object_id,
            display_name: receipt.display_name,
            plaintext_size,
            deduplicated,
            staged_file_removed,
        })
    }

    async fn execute_universal_import(
        &self,
        transaction_id: ImportTransactionId,
        request: UniversalStagedImportRequest,
        admitted_name: crate::storage::file_policy::AllowedFileName,
        canonical_path: PathBuf,
        cancellation: CancellationToken,
    ) -> Result<WorkspaceStagedImportReceipt, StagedImportError>
    where
        C: Send + Sync + 'static,
    {
        transition(&self.pool, transaction_id, ImportState::Validating).await?;
        cancel_if_requested(&self.pool, transaction_id, &cancellation).await?;
        transition(&self.pool, transaction_id, ImportState::Copying).await?;
        let path_for_read = canonical_path.clone();
        let maximum = self.max_import_bytes;
        let expected_size = request.expected_size;
        let bytes = tokio::task::spawn_blocking(move || {
            read_bounded_staged_file(&path_for_read, maximum, expected_size)
        })
        .await
        .map_err(|error| StagedImportError::BlockingTask(error.to_string()))??;
        ImportJournalRepository::record_progress(&self.pool, transaction_id, bytes.len() as u64)
            .await?;
        cancel_if_requested(&self.pool, transaction_id, &cancellation).await?;
        transition(&self.pool, transaction_id, ImportState::Hashing).await?;
        validate_text_content(&bytes)?;
        cancel_if_requested(&self.pool, transaction_id, &cancellation).await?;
        transition(&self.pool, transaction_id, ImportState::Encrypting).await?;
        cancel_if_requested(&self.pool, transaction_id, &cancellation).await?;
        let object_store = Arc::clone(&self.object_store);
        let stored_object = tokio::task::spawn_blocking(move || object_store.put(&bytes))
            .await
            .map_err(|error| StagedImportError::BlockingTask(error.to_string()))??;
        transition(&self.pool, transaction_id, ImportState::CommittingObject).await?;
        transition(&self.pool, transaction_id, ImportState::CommittingNode).await?;
        let plaintext_size = stored_object.plaintext_size;
        let deduplicated = stored_object.deduplicated;
        let detected_format = match &admitted_name.rule {
            FileAdmissionRule::Extension(extension) => extension.to_string(),
            FileAdmissionRule::ExactName(name) => format!("exact:{name}"),
        };
        let receipt = ImportCommitRepository::commit(
            &self.pool,
            ImportCommitRequest {
                transaction_id,
                authorization: ImportAuthorization::Universal,
                admitted_name,
                stored_object,
                detected_format,
                detected_mime: request
                    .detected_mime
                    .filter(|value| !value.trim().is_empty()),
                detected_encoding: Some("utf-8".to_string()),
                language_id: None,
                encryption_version: self.object_store.encryption_version(),
                duplicate_policy: request.duplicate_policy,
            },
        )
        .await?;
        transition(&self.pool, transaction_id, ImportState::Completed).await?;
        let cleanup_path = canonical_path;
        let staged_file_removed =
            tokio::task::spawn_blocking(move || fs::remove_file(cleanup_path).is_ok())
                .await
                .unwrap_or(false);
        Ok(WorkspaceStagedImportReceipt {
            transaction_id,
            node_id: receipt.node_id,
            object_id: receipt.object_id,
            display_name: receipt.display_name,
            plaintext_size,
            deduplicated,
            staged_file_removed,
        })
    }
}

#[async_trait::async_trait]
impl<C> StagedFileImporter for WorkspaceStagedImportService<C>
where
    C: ObjectCipher + Send + Sync + 'static,
{
    async fn import_workspace_file(
        &self,
        request: WorkspaceStagedImportRequest,
        cancellation: CancellationToken,
    ) -> Result<WorkspaceStagedImportReceipt, StagedImportError> {
        let admitted_name = admit_file_name(&request.original_filename)?;
        if request.duplicate_policy == DuplicatePolicy::ReplaceWithNewVersion {
            return Err(StagedImportError::InvalidConfiguration);
        }

        let inspected = inspect_staged_file(
            &self.staging_root,
            &request.staged_path,
            self.max_import_bytes,
            request.expected_size,
        )?;
        let workspace =
            UniversalStorageRepository::ensure_workspace(&self.pool, request.chat_id.clone())
                .await?;
        let transaction_id = ImportJournalRepository::create(
            &self.pool,
            workspace.scope_id,
            workspace.uploaded_files_node_id(),
            admitted_name.display_name.clone(),
            inspected.relative_path,
            Some(inspected.size),
            request.source_uri_fingerprint.clone(),
        )
        .await?;

        let result = self
            .execute_import(
                transaction_id,
                request,
                admitted_name,
                inspected.canonical_path,
                cancellation,
            )
            .await;
        if let Err(error) = &result {
            if !matches!(error, StagedImportError::Cancelled) {
                let _ = ImportJournalRepository::transition(
                    &self.pool,
                    transaction_id,
                    ImportState::Failed,
                    Some(error.code().to_string()),
                    None,
                )
                .await;
            }
        }
        result
    }

    async fn import_universal_file(
        &self,
        request: UniversalStagedImportRequest,
        cancellation: CancellationToken,
    ) -> Result<WorkspaceStagedImportReceipt, StagedImportError> {
        let admitted_name = admit_file_name(&request.original_filename)?;
        if request.duplicate_policy == DuplicatePolicy::ReplaceWithNewVersion {
            return Err(StagedImportError::InvalidConfiguration);
        }
        let inspected = inspect_staged_file(
            &self.staging_root,
            &request.staged_path,
            self.max_import_bytes,
            request.expected_size,
        )?;
        let universal = UniversalStorageRepository::ensure_universal_storage(&self.pool).await?;
        validate_universal_import_parent(&self.pool, universal.scope_id, request.parent_node_id)
            .await?;
        let transaction_id = ImportJournalRepository::create(
            &self.pool,
            universal.scope_id,
            request.parent_node_id,
            admitted_name.display_name.clone(),
            inspected.relative_path,
            Some(inspected.size),
            request.source_uri_fingerprint.clone(),
        )
        .await?;
        let result = self
            .execute_universal_import(
                transaction_id,
                request,
                admitted_name,
                inspected.canonical_path,
                cancellation,
            )
            .await;
        if let Err(error) = &result {
            if !matches!(error, StagedImportError::Cancelled) {
                let _ = ImportJournalRepository::transition(
                    &self.pool,
                    transaction_id,
                    ImportState::Failed,
                    Some(error.code().to_string()),
                    None,
                )
                .await;
            }
        }
        result
    }
}

async fn validate_universal_import_parent(
    pool: &DatabasePool,
    scope_id: StorageScopeId,
    parent_node_id: StorageNodeId,
) -> Result<(), StagedImportError> {
    pool.with_conn(move |connection| {
        let valid: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM storage_nodes n \
             JOIN storage_scopes s ON s.scope_id = n.scope_id \
             WHERE n.node_id = ?1 AND n.scope_id = ?2 AND n.node_type = 'directory' \
               AND n.state = 'active' AND s.scope_type = 'universal' AND s.state = 'active' \
               AND COALESCE(n.system_role, '') != 'trash')",
            rusqlite::params![parent_node_id.to_string(), scope_id.to_string()],
            |row| row.get::<_, i64>(0).map(|value| value != 0),
        )?;
        if !valid {
            return Err(DbError::Domain(MukeiError::Invariant(
                "universal import parent is not an active user-writable directory".into(),
            )));
        }
        Ok::<_, DbError>(())
    })
    .await?;
    Ok(())
}

struct InspectedStagedFile {
    canonical_path: PathBuf,
    relative_path: String,
    size: u64,
}

fn inspect_staged_file(
    staging_root: &Path,
    requested_path: &Path,
    maximum: u64,
    expected_size: Option<u64>,
) -> Result<InspectedStagedFile, StagedImportError> {
    let candidate = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        staging_root.join(requested_path)
    };
    let symlink_metadata =
        fs::symlink_metadata(&candidate).map_err(|_| StagedImportError::MissingStagedFile)?;
    if symlink_metadata.file_type().is_symlink() {
        return Err(StagedImportError::UnsafeStagingPath);
    }
    let canonical_path =
        fs::canonicalize(candidate).map_err(|_| StagedImportError::MissingStagedFile)?;
    if !canonical_path.starts_with(staging_root) {
        return Err(StagedImportError::UnsafeStagingPath);
    }
    let metadata =
        fs::metadata(&canonical_path).map_err(|_| StagedImportError::MissingStagedFile)?;
    if !metadata.is_file() {
        return Err(StagedImportError::MissingStagedFile);
    }
    if metadata.len() > maximum {
        return Err(StagedImportError::FileTooLarge);
    }
    if expected_size.is_some_and(|expected| expected != metadata.len()) {
        return Err(StagedImportError::SizeMismatch);
    }
    let relative_path = canonical_path
        .strip_prefix(staging_root)
        .map_err(|_| StagedImportError::UnsafeStagingPath)?
        .to_str()
        .ok_or(StagedImportError::UnsafeStagingPath)?
        .to_string();
    Ok(InspectedStagedFile {
        canonical_path,
        relative_path,
        size: metadata.len(),
    })
}

fn read_bounded_staged_file(
    path: &Path,
    maximum: u64,
    expected_size: Option<u64>,
) -> Result<Vec<u8>, StagedImportError> {
    let file = File::open(path).map_err(|_| StagedImportError::MissingStagedFile)?;
    let initial_size = file
        .metadata()
        .map_err(|_| StagedImportError::MissingStagedFile)?
        .len();
    if initial_size > maximum {
        return Err(StagedImportError::FileTooLarge);
    }
    if expected_size.is_some_and(|expected| expected != initial_size) {
        return Err(StagedImportError::SizeMismatch);
    }
    let capacity =
        usize::try_from(initial_size.min(maximum)).map_err(|_| StagedImportError::FileTooLarge)?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(ObjectStoreError::Io)?;
    if bytes.len() as u64 > maximum {
        return Err(StagedImportError::FileTooLarge);
    }
    let final_size = fs::metadata(path)
        .map_err(|_| StagedImportError::StagedFileChanged)?
        .len();
    if final_size != initial_size || bytes.len() as u64 != initial_size {
        return Err(StagedImportError::StagedFileChanged);
    }
    Ok(bytes)
}

fn validate_text_content(bytes: &[u8]) -> Result<(), StagedImportError> {
    if bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
        return Err(StagedImportError::NonUtf8Text);
    }
    Ok(())
}

async fn transition(
    pool: &DatabasePool,
    transaction_id: ImportTransactionId,
    next: ImportState,
) -> Result<(), StagedImportError> {
    ImportJournalRepository::transition(pool, transaction_id, next, None, None)
        .await
        .map_err(StagedImportError::Storage)
}

async fn cancel_if_requested(
    pool: &DatabasePool,
    transaction_id: ImportTransactionId,
    cancellation: &CancellationToken,
) -> Result<(), StagedImportError> {
    if !cancellation.is_cancelled() {
        return Ok(());
    }
    ImportJournalRepository::transition(
        pool,
        transaction_id,
        ImportState::CancelRequested,
        Some("storage_import_cancelled".to_string()),
        None,
    )
    .await?;
    ImportJournalRepository::transition(
        pool,
        transaction_id,
        ImportState::Cancelled,
        Some("storage_import_cancelled".to_string()),
        None,
    )
    .await?;
    Err(StagedImportError::Cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrations::Migrator;
    use crate::storage::object_store::ObjectCipher;
    use crate::storage::pool::{DbError, PooledConnectionExt};
    use sha2::{Digest, Sha256};

    struct TestCipher;

    impl ObjectCipher for TestCipher {
        fn version(&self) -> u32 {
            1
        }

        fn seal(&self, plaintext: &[u8], associated_data: &[u8]) -> Result<Vec<u8>, String> {
            let mask = Sha256::digest(associated_data);
            Ok(plaintext
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % mask.len()])
                .collect())
        }

        fn open(&self, ciphertext: &[u8], associated_data: &[u8]) -> Result<Vec<u8>, String> {
            self.seal(ciphertext, associated_data)
        }
    }

    async fn service(
        root: &Path,
    ) -> (
        Arc<DatabasePool>,
        WorkspaceStagedImportService<TestCipher>,
        PathBuf,
    ) {
        let pool = Arc::new(DatabasePool::open(&root.join("storage.db")).unwrap());
        Migrator::embedded().apply_pending(&pool).await.unwrap();
        let staging = root.join("staging");
        let objects = root.join("objects");
        let store = Arc::new(ImmutableObjectStore::open(objects, TestCipher).unwrap());
        let service = WorkspaceStagedImportService::new(
            Arc::clone(&pool),
            store,
            &staging,
            DEFAULT_MAX_STAGED_IMPORT_BYTES,
        )
        .unwrap();
        (pool, service, staging)
    }

    fn request(staged_path: PathBuf, name: &str) -> WorkspaceStagedImportRequest {
        WorkspaceStagedImportRequest {
            chat_id: ChatId::parse("chat-1").unwrap(),
            staged_path,
            original_filename: name.to_string(),
            detected_mime: Some("text/plain".to_string()),
            expected_size: None,
            duplicate_policy: DuplicatePolicy::RenameNewEntry,
            source_uri_fingerprint: Some("fingerprint".to_string()),
        }
    }

    #[tokio::test]
    async fn imports_into_uploaded_files_and_removes_staging_plaintext() {
        let root = tempfile::tempdir().unwrap();
        let (pool, service, staging) = service(root.path()).await;
        let staged = staging.join("notes.txt");
        fs::write(&staged, b"hello workspace").unwrap();

        let receipt = service
            .import_workspace_file(
                request(staged.clone(), "notes.txt"),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(receipt.display_name, "notes.txt");
        assert_eq!(receipt.plaintext_size, 15);
        assert!(receipt.staged_file_removed);
        assert!(!staged.exists());
        let transaction_id = receipt.transaction_id.to_string();
        let state: String = pool
            .with_conn(move |connection| {
                let state = connection.query_row(
                    "SELECT state FROM import_transactions WHERE transaction_id = ?1",
                    [transaction_id],
                    |row| row.get(0),
                )?;
                Ok::<_, DbError>(state)
            })
            .await
            .unwrap();
        assert_eq!(state, "completed");
    }

    #[tokio::test]
    async fn duplicate_content_reuses_ciphertext_and_renames_sibling() {
        let root = tempfile::tempdir().unwrap();
        let (_pool, service, staging) = service(root.path()).await;
        let first = staging.join("first.txt");
        fs::write(&first, b"same content").unwrap();
        let first_receipt = service
            .import_workspace_file(request(first, "notes.txt"), CancellationToken::new())
            .await
            .unwrap();
        assert!(!first_receipt.deduplicated);

        let second = staging.join("second.txt");
        fs::write(&second, b"same content").unwrap();
        let second_receipt = service
            .import_workspace_file(request(second, "notes.txt"), CancellationToken::new())
            .await
            .unwrap();
        assert!(second_receipt.deduplicated);
        assert_eq!(second_receipt.display_name, "notes (2).txt");
        assert_eq!(first_receipt.object_id, second_receipt.object_id);
    }

    #[tokio::test]
    async fn cancellation_is_journaled_before_encryption() {
        let root = tempfile::tempdir().unwrap();
        let (pool, service, staging) = service(root.path()).await;
        let staged = staging.join("cancel.txt");
        fs::write(&staged, b"cancel me").unwrap();
        let token = CancellationToken::new();
        token.cancel();

        let error = service
            .import_workspace_file(request(staged, "cancel.txt"), token)
            .await
            .unwrap_err();
        assert!(matches!(error, StagedImportError::Cancelled));
        let state: String = pool
            .with_conn(|connection| {
                let state = connection.query_row(
                    "SELECT state FROM import_transactions ORDER BY created_at DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )?;
                Ok::<_, DbError>(state)
            })
            .await
            .unwrap();
        assert_eq!(state, "cancelled");
    }

    #[tokio::test]
    async fn rejects_unsupported_names_and_paths_outside_staging_jail() {
        let root = tempfile::tempdir().unwrap();
        let (_pool, service, staging) = service(root.path()).await;
        let inside = staging.join("report.pdf");
        fs::write(&inside, b"not allowed").unwrap();
        let error = service
            .import_workspace_file(request(inside, "report.pdf"), CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(error, StagedImportError::FileAdmission(_)));

        let outside = root.path().join("outside.txt");
        fs::write(&outside, b"outside").unwrap();
        let error = service
            .import_workspace_file(request(outside, "outside.txt"), CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(error, StagedImportError::UnsafeStagingPath));
    }

    #[tokio::test]
    async fn rejects_binary_content_despite_allowed_extension() {
        let root = tempfile::tempdir().unwrap();
        let (pool, service, staging) = service(root.path()).await;
        let staged = staging.join("binary.txt");
        fs::write(&staged, [0xff, 0x00, 0x01]).unwrap();

        let error = service
            .import_workspace_file(request(staged, "binary.txt"), CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(error, StagedImportError::NonUtf8Text));
        let state: String = pool
            .with_conn(|connection| {
                let state = connection.query_row(
                    "SELECT state FROM import_transactions ORDER BY created_at DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )?;
                Ok::<_, DbError>(state)
            })
            .await
            .unwrap();
        assert_eq!(state, "failed");
    }
}
