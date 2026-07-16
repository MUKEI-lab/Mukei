//! `mukei_core::storage` — TRD §6 / BS v1.2.
//!
//! Every asynchronous database call is routed through the pooled blocking
//! boundary. SQLCipher keys are supplied only by the Android secure bootstrap.

pub mod file_policy;
pub mod object_store;
pub mod universal;

#[cfg(feature = "rusqlite")]
pub mod audit_log;
#[cfg(feature = "rusqlite")]
pub mod conversation;
#[cfg(feature = "rusqlite")]
pub mod download_jobs;
#[cfg(feature = "rusqlite")]
pub mod import_journal;
#[cfg(feature = "rusqlite")]
pub mod migrations;
#[cfg(feature = "rusqlite")]
pub mod pool;
#[cfg(feature = "rusqlite")]
pub mod recovery;
#[cfg(feature = "rusqlite")]
pub mod runtime_projection;
#[cfg(feature = "rusqlite")]
pub mod saas;
#[cfg(feature = "rusqlite")]
pub mod saf;
#[cfg(feature = "rusqlite")]
pub mod settings;
#[cfg(feature = "rusqlite")]
pub mod trash_repository;
#[cfg(feature = "rusqlite")]
pub mod ui_session;
#[cfg(feature = "rusqlite")]
pub mod universal_repository;
#[cfg(feature = "rusqlite")]
pub mod version_repository;

pub use file_policy::{
    admit_file_name, AllowedFileName, FileAdmissionError, FileAdmissionRule,
    ALLOWED_EXACT_NAMES, ALLOWED_EXTENSIONS, FILE_POLICY_VERSION, MAX_FILENAME_BYTES,
};
pub use object_store::{
    ImmutableObjectStore, ObjectCipher, ObjectStoreError, StoredObject,
};
pub use universal::{
    ChatId, DuplicatePolicy, FileVersionId, ImportTarget, ImportTransactionId,
    PlannedDirectory, StorageDomainError, StorageNodeId, StorageNodeKind, StorageNodeState,
    StorageObjectId, StorageScopeId, StorageScopeType, SystemDirectoryRole,
    WorkspaceAccessContext, WorkspaceId, WorkspaceLayout, UNIVERSAL_STORAGE_NAME,
};

#[cfg(feature = "rusqlite")]
pub use audit_log::{AuditChainStatus, AuditEntry, AuditLogReader, AuditLogWriter};
#[cfg(feature = "rusqlite")]
pub use conversation::{
    ConversationRecord, ConversationRepository, ConversationSummary, MessageRecord, MessageStatus,
    PersistedTurn, TimelinePage, TimelineRow,
};
#[cfg(feature = "rusqlite")]
pub use download_jobs::{
    DownloadJobRecord, DownloadJobRepository, DownloadJobStatus, DownloadReservation,
};
#[cfg(feature = "rusqlite")]
pub use import_journal::{
    ImportJournalRepository, ImportState, ImportTransactionRecord,
};
#[cfg(feature = "rusqlite")]
pub use migrations::{
    MigrationBackup, MigrationRecord, Migrator, MIGRATIONS_DIR, MIGRATION_FILE_PREFIX,
};
#[cfg(feature = "rusqlite")]
pub use pool::{
    DatabaseEncryptionStatus, DatabaseOpenResult, DatabasePool, DbError, PooledConnectionExt,
};
#[cfg(feature = "rusqlite")]
pub use recovery::{InterruptedTurn, RecoveryAttempt, RecoveryMode, RecoveryState, RecoveryStore};
#[cfg(feature = "rusqlite")]
pub use runtime_projection::{RuntimeProjectionRepository, RuntimeProjectionRow};
#[cfg(feature = "rusqlite")]
pub use saas::{
    EntitlementRepository, MembershipRepository, QuotaPolicyRepository, RecordApplyOutcome,
    SnapshotApplyOutcome, SubscriptionRepository, TenantWorkspaceRepository, UsageAppendOutcome,
    UsageLedgerRepository,
};
#[cfg(feature = "rusqlite")]
pub use saf::{DocumentProjection, SafCleanupPlan, SafRegistry, SafTokenRow};
#[cfg(feature = "rusqlite")]
pub use settings::{PreferenceRecord, PreferenceValue, SecretRefRecord, SettingsRepository};
#[cfg(feature = "rusqlite")]
pub use trash_repository::{RestoreReceipt, TrashReceipt, TrashRepository};
#[cfg(feature = "rusqlite")]
pub use ui_session::{
    UiDraftRecord, UiSessionRecord, UiSessionRepository, DEFAULT_UI_PROFILE,
    UI_SESSION_SCHEMA_VERSION,
};
#[cfg(feature = "rusqlite")]
pub use universal_repository::{
    PersistedSystemDirectory, PersistedUniversalStorage, PersistedWorkspace,
    UniversalStorageRepository,
};
#[cfg(feature = "rusqlite")]
pub use version_repository::{
    FileVersionRepository, NewFileVersion, PersistedFileVersion, VersionCreator,
};

#[cfg(feature = "tokio")]
pub mod model_download;
#[cfg(feature = "tokio")]
pub use model_download::{run_download, verify_file_sha256, DownloadEvent, DownloadRequest};
#[cfg(feature = "tokio")]
pub mod quota;
#[cfg(feature = "tokio")]
pub use quota::{StorageQuotaManager, StorageQuotaPolicy, StorageUsage};
