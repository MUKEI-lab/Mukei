//! `mukei_core::storage` — TRD §6 / BS v1.2.
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
pub mod saf;

#[cfg(feature = "rusqlite")]
pub use migrations::{Migrator, MigrationRecord, MIGRATIONS_DIR, MIGRATION_FILE_PREFIX};
#[cfg(feature = "rusqlite")]
pub use pool::{DatabasePool, DbError, PooledConnectionExt};
#[cfg(feature = "rusqlite")]
pub use saf::{SafRegistry, SafTokenRow};
