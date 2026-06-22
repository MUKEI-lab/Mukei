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
//!   - `pool`     — `r2d2`-backed `!Send` SQLite pool.
//!                 All async paths MUST `spawn_blocking` (TRD §2.4
//!                 "Golden Rule").
//!   - `migrations` — strictly versioned, append-only SQL migrations.
//!   - `saf`      — SAF URI grant registry (TRD §5.4).
//!
//! This module is gated on the `rusqlite` feature so it can be unit-tested
//! even on hosts where SQLite is not desirable.

#[cfg(feature = "rusqlite")]
pub mod migrations;
#[cfg(feature = "rusqlite")]
pub mod pool;
#[cfg(feature = "rusqlite")]
pub mod recovery;
#[cfg(feature = "rusqlite")]
pub mod saf;

#[cfg(feature = "rusqlite")]
pub use migrations::{MigrationRecord, Migrator, MIGRATIONS_DIR, MIGRATION_FILE_PREFIX};
#[cfg(feature = "rusqlite")]
pub use pool::{DatabasePool, DbError, PooledConnectionExt};
#[cfg(feature = "rusqlite")]
pub use recovery::{RecoveryState, RecoveryStore};
#[cfg(feature = "rusqlite")]
pub use saf::{SafRegistry, SafTokenRow};
