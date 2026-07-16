//! `mukei_core::storage` — TRD §6 / BS v1.2.
//!
//! Every asynchronous database call is routed through the pooled blocking
//! boundary. SQLCipher keys are supplied only by the Android secure bootstrap.

#[cfg(feature = "rusqlite")]
pub mod audit_log;
#[cfg(feature = "rusqlite")]
pub mod conversation;
#[cfg(feature = "rusqlite")]
pub mod download_jobs;
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
pub mod ui_session;

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
pub use ui_session::{
    UiDraftRecord, UiSessionRecord, UiSessionRepository, DEFAULT_UI_PROFILE,
    UI_SESSION_SCHEMA_VERSION,
};

#[cfg(feature = "tokio")]
pub mod model_download;
#[cfg(feature = "tokio")]
pub use model_download::{run_download, verify_file_sha256, DownloadEvent, DownloadRequest};
#[cfg(feature = "tokio")]
pub mod quota;
#[cfg(feature = "tokio")]
pub use quota::{StorageQuotaManager, StorageQuotaPolicy, StorageUsage};
