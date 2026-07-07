//! `mukei_core::storage` — TRD §6 / BS v1.2.
//!
//! # Invariants
//!
//! - Every async DB call route MUST pass through [`pool::PooledConnectionExt::with_conn`]
//!   so the synchronous `rusqlite` work runs inside `spawn_blocking`
//!   (TRD §2.4 Golden Rule). A bare `pool.get()` from async code is a bug.
//! - Migrations are append-only, sequentially numbered (V001, V002, …)
//!   and idempotent. Boot refuses to start if `migrations_applied` shows
//!   an out-of-order set (`MukeiError::MigrationOrderConflict`). See
//!   BS §3 and the `migrations/` directory.
//! - The SAF registry ([`saf::SafRegistry`]) is the **only** source of
//!   truth for path resolution. The `read_file` tool MUST NOT accept
//!   bare filesystem paths.
//! - SQLCipher key material never crosses the FFI as plaintext; the
//!   bridge crate hands a wrapped blob to the unwrap step under
//!   `feature = "android_keystore"`.
//!
//! Contains:
//! - `pool` — `r2d2`-backed `!Send` SQLite pool. All async paths MUST
//!   `spawn_blocking` (TRD §2.4 "Golden Rule").
//! - `migrations` — strictly versioned, append-only SQL migrations.
//! - `saf` — SAF URI grant registry (TRD §5.4).
//!
//! This module is gated on the `rusqlite` feature so it can be unit-tested
//! even on hosts where SQLite is not desirable.

#[cfg(feature = "rusqlite")]
pub mod audit_log;
#[cfg(feature = "rusqlite")]
pub mod conversation;
#[cfg(feature = "rusqlite")]
pub mod migrations;
#[cfg(feature = "rusqlite")]
pub mod pool;
#[cfg(feature = "rusqlite")]
pub mod recovery;
#[cfg(feature = "rusqlite")]
pub mod saf;

#[cfg(feature = "rusqlite")]
pub use audit_log::{AuditChainStatus, AuditEntry, AuditLogReader, AuditLogWriter};
#[cfg(feature = "rusqlite")]
pub use conversation::{
    ConversationRecord, ConversationRepository, MessageRecord, MessageStatus, PersistedTurn,
};
#[cfg(feature = "rusqlite")]
pub use migrations::{MigrationRecord, Migrator, MIGRATIONS_DIR, MIGRATION_FILE_PREFIX};
#[cfg(feature = "rusqlite")]
pub use pool::{
    DatabaseEncryptionStatus, DatabaseOpenResult, DatabasePool, DbError, PooledConnectionExt,
};
#[cfg(feature = "rusqlite")]
pub use recovery::{RecoveryState, RecoveryStore};
#[cfg(feature = "rusqlite")]
pub use saf::{SafRegistry, SafTokenRow};

// TRD §8.1 / PRD REQ-MOD-01 — on-device GGUF downloader. Independent of
// the rusqlite-gated persistence layer because testers must be able to
// fetch the model even on builds without encrypted SQLite (e.g. early
// desktop runs). The real reqwest-backed implementation is gated on
// `network`; the sandbox build gets a stub plus the validation /
// hashing / event types so unit tests still cover them.
#[cfg(feature = "tokio")]
pub mod model_download;
#[cfg(feature = "tokio")]
pub use model_download::{run_download, verify_file_sha256, DownloadEvent, DownloadRequest};
