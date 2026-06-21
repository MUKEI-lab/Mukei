//! `mukei_core::storage::pool` — TRD §6 / §2.4 / BS v1.2.
//!
//! `DatabasePool` is a `r2d2`-backed pool of SQLite connections.
//! Crucially, `rusqlite::Connection` is **`!Send + !Sync`** — every
//! async path that touches the pool MUST wrap the synchronous DB code
//! in `tokio::task::spawn_blocking`. The extension trait
//! [`PooledConnectionExt::with_conn`] provides a golden-rule helper so
//! no caller can accidentally drift back to the
//! `let conn = pool.get().await` panic footgun.
//!
//! The actual SQLite library is feature-gated (`rusqlite`) so this
//! crate still compiles on hosts without SQLite.

#[cfg(feature = "rusqlite")]
use std::path::Path;
#[cfg(feature = "rusqlite")]
use std::time::Duration;

#[cfg(feature = "rusqlite")]
use r2d2::ManageConnection;

#[cfg(feature = "rusqlite")]
use crate::error::{MukeiError, Result};

/// Pool-specific error mapped into [`MukeiError`].
#[cfg(feature = "rusqlite")]
#[derive(thiserror::Error, Debug)]
pub enum DbError {
    #[error("r2d2 pool timed out after {0:?}")]
    PoolTimeout(Duration),
    #[error("rusqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("connection manager error: {0}")]
    Manager(String),
}

#[cfg(feature = "rusqlite")]
impl From<DbError> for MukeiError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::Sqlite(_)        => MukeiError::DatabaseInitFailed(e.to_string()),
            DbError::Manager(_)       => MukeiError::DatabaseInitFailed(e.to_string()),
            DbError::PoolTimeout(_)   => MukeiError::DatabaseInitFailed(e.to_string()),
        }
    }
}

#[cfg(feature = "rusqlite")]
mod platform {
    //! Target-conditional import of `rusqlite` features (bundled,
    //! WAL, SQLCipher).
    pub use rusqlite::*;
    pub type Conn = Connection;
}

#[cfg(feature = "rusqlite")]
pub type Conn = platform::Conn;

/// Newtype around an `r2d2::Pool<SqliteConnectionManager>`. The newtype
/// keeps `Send + Sync` requirements explicit and forces callers
/// through the safe extension trait.
#[cfg(feature = "rusqlite")]
pub struct DatabasePool {
    inner: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
}

#[cfg(feature = "rusqlite")]
impl DatabasePool {
    /// Open a SQLite pool at the given path. WAL + bundled enabled via
    /// the workspace `rusqlite` feature flag (TRD §6 / Cargo.toml).
    pub fn open(path: &Path) -> Result<Self> {
        let manager = r2d2_sqlite::SqliteConnectionManager::file(path)
            .with_init(|c| {
                c.pragma_update(None, "journal_mode", "WAL")?;
                c.pragma_update(None, "synchronous", "NORMAL")?;
                c.pragma_update(None, "foreign_keys", "ON")?;
                c.pragma_update(None, "busy_timeout", "5000")?;
                Ok(())
            });
        let pool = r2d2::Pool::builder()
            .max_size(8)
            .build(manager)
            .map_err(|e| MukeiError::DatabaseInitFailed(format!("pool build: {e}")))?;
        Ok(Self { inner: pool })
    }

    /// Acquire one connection synchronously. Only callable from
    /// `spawn_blocking` contexts.
    pub fn blocking_acquire(&self) -> std::result::Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, DbError> {
        self.inner.get().map_err(|e| {
            let dur = Duration::from_secs(5);
            match e {
                r2d2::Error::Timeout(_) => DbError::PoolTimeout(dur),
                _ => DbError::Manager(e.to_string()),
            }
        })
    }

    /// Direct access to the r2d2 pool (escape hatch).
    pub fn raw(&self) -> &r2d2::Pool<r2d2_sqlite::SqliteConnectionManager> {
        &self.inner
    }
}

/// Extension trait that enforces the §2.4 spawn-blocking rule at the
/// type level. Use this from async code:
/// ```no_run
/// # async fn demo(pool: mukei_core::storage::DatabasePool) -> Result<(), mukei_core::error::MukeiError> {
/// use mukei_core::storage::PooledConnectionExt;
/// let rows = pool
///     .with_conn(|c| {
///         let mut s = c.prepare("SELECT id FROM migrations_applied")?;
///         let r: Vec<i64> = s.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
///         Ok(r)
///     })
///     .await?;
/// # Ok(()) }
/// ```
#[cfg(feature = "rusqlite")]
#[async_trait::async_trait]
pub trait PooledConnectionExt {
    /// Run `f` on a freshly-acquired connection **inside**
    /// `spawn_blocking`. This is the only safe async→sync bridge.
    async fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Conn) -> std::result::Result<T, DbError> + Send + 'static,
        T: Send + 'static;
}

#[cfg(feature = "rusqlite")]
#[async_trait::async_trait]
impl PooledConnectionExt for DatabasePool {
    async fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Conn) -> std::result::Result<T, DbError> + Send + 'static,
        T: Send + 'static,
    {
        let pool = self.inner.clone();
        tokio::task::spawn_blocking(move || -> std::result::Result<T, MukeiError> {
            let mut conn = pool.get().map_err(DbError::from)?;
            f(&mut conn).map_err(MukeiError::from)
        })
        .await
        .map_err(|e| MukeiError::BlockingJoinFailed(e.to_string()))?
    }
}

// ------- Non-rusqlite stub: keep crate compilable everywhere --------
#[cfg(not(feature = "rusqlite"))]
pub struct DatabasePool;

#[cfg(not(feature = "rusqlite"))]
impl DatabasePool {
    pub fn open(_path: &std::path::Path) -> crate::error::Result<Self> {
        Ok(Self)
    }
}

// --------------------------------------------------

#[cfg(all(test, feature = "rusqlite"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn with_conn_runs_sqlite_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mukei.db");
        let pool = DatabasePool::open(&path).unwrap();

        // Create a tiny table inside the spawn-blocking wrapper.
        let _: i64 = pool
            .with_conn(|c| {
                c.execute_batch("CREATE TABLE t (n INTEGER)")?;
                c.execute("INSERT INTO t (n) VALUES (?1)", [42_i64])?;
                let n: i64 = c.query_row("SELECT n FROM t", [], |r| r.get(0))?;
                Ok(n)
            })
            .await
            .unwrap();
    }
}
