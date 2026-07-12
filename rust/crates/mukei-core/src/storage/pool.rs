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
use crate::error::{MukeiError, Result};

#[cfg(all(feature = "rusqlite", any(test, feature = "sqlcipher")))]
const SQLITE_PLAIN_HEADER: &[u8; 16] = b"SQLite format 3\0";

#[cfg(feature = "rusqlite")]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DatabaseEncryptionStatus {
    Encrypted,
    Unavailable,
    InvalidKey,
    Corrupted,
    MigrationRequired,
}

#[cfg(feature = "rusqlite")]
pub struct DatabaseOpenResult {
    pub pool: DatabasePool,
    pub encryption_status: DatabaseEncryptionStatus,
}

#[cfg(all(feature = "rusqlite", any(test, feature = "sqlcipher")))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum DatabaseHeaderState {
    Missing,
    Empty,
    PlainSqlite,
    NotPlainSqlite,
}

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
    /// Domain error produced inside a blocking database operation. This
    /// preserves the original stable error code instead of flattening it
    /// into ERR_DB_INIT at the async pool boundary.
    #[error(transparent)]
    Domain(#[from] MukeiError),
}

#[cfg(feature = "rusqlite")]
impl From<DbError> for MukeiError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::Sqlite(_) => MukeiError::DatabaseInitFailed(e.to_string()),
            DbError::Manager(_) => MukeiError::DatabaseInitFailed(e.to_string()),
            DbError::PoolTimeout(_) => MukeiError::DatabaseInitFailed(e.to_string()),
            DbError::Domain(error) => error,
        }
    }
}

#[cfg(feature = "rusqlite")]
impl From<r2d2::Error> for DbError {
    fn from(e: r2d2::Error) -> Self {
        DbError::Manager(e.to_string())
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
    /// Open a plain SQLite pool at `path` (no encryption).
    ///
    /// WAL + bundled SQLite enabled via the workspace `rusqlite`
    /// feature flag (TRD §6 / Cargo.toml). Use
    /// [`Self::open_with_cipher_key`] for the encrypted production path
    /// (PRD REQ-SEC-19).
    pub fn open(path: &Path) -> Result<Self> {
        let manager = r2d2_sqlite::SqliteConnectionManager::file(path).with_init(|c| {
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

    /// Open a SQLCipher pool at `path` using the supplied unwrapped
    /// key bytes (TRD §6.2 / PRD REQ-SEC-19).
    ///
    /// # Invariants
    /// - `unwrapped_key` MUST come straight from the Android Keystore
    ///   unwrap step (or the desktop keyring equivalent). The bridge
    ///   crate is responsible for that step.
    /// - The key bytes are bound via `PRAGMA key = x'<hex>'` so they
    ///   never appear in a query plan / log line.
    /// - Every pooled connection receives the key inside `with_init`.
    ///   SQLCipher requires this per connection; keying only the first
    ///   r2d2 connection leaves later pool members unable to read the DB.
    /// - The pool holds the key in a zeroizing wrapper for its lifetime
    ///   and zeroizes per-connection hex renderings immediately after use.
    /// - Only gated behind `feature = "sqlcipher"` because plain
    ///   `rusqlite` builds do not understand `PRAGMA key`. On non-cipher
    ///   builds the bridge should call [`Self::open`] instead.
    #[cfg(feature = "sqlcipher")]
    pub fn open_with_cipher_key(
        path: &Path,
        unwrapped_key: zeroize::Zeroizing<Vec<u8>>,
    ) -> Result<Self> {
        Self::open_with_cipher_key_result(path, unwrapped_key).map(|result| result.pool)
    }

    /// Status-returning SQLCipher open path. Production boot should use
    /// this when it needs to expose the encryption state to the bridge.
    #[cfg(feature = "sqlcipher")]
    pub fn open_with_cipher_key_result(
        path: &Path,
        unwrapped_key: zeroize::Zeroizing<Vec<u8>>,
    ) -> Result<DatabaseOpenResult> {
        use zeroize::Zeroizing;

        if unwrapped_key.is_empty() {
            return Err(MukeiError::DatabaseEncryptionInvalidKey);
        }

        match inspect_database_header(path)? {
            DatabaseHeaderState::PlainSqlite => {
                return Err(MukeiError::DatabaseEncryptionMigrationRequired);
            }
            DatabaseHeaderState::Missing
            | DatabaseHeaderState::Empty
            | DatabaseHeaderState::NotPlainSqlite => {}
        }

        let key = std::sync::Arc::new(unwrapped_key);

        let key_for_init = key.clone();
        let manager = r2d2_sqlite::SqliteConnectionManager::file(path).with_init(move |c| {
            ensure_sqlcipher_available(c)?;
            let hex_key = Zeroizing::new(
                key_for_init
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            );
            let pragma_value = Zeroizing::new(format!("x'{}'", &*hex_key));
            c.pragma_update(None, "key", &*pragma_value)?;
            verify_keyed_database(c)?;
            c.pragma_update(None, "journal_mode", "WAL")?;
            c.pragma_update(None, "synchronous", "NORMAL")?;
            c.pragma_update(None, "foreign_keys", "ON")?;
            c.pragma_update(None, "busy_timeout", "5000")?;
            Ok(())
        });
        let pool = r2d2::Pool::builder()
            .max_size(8)
            .build(manager)
            .map_err(map_sqlcipher_pool_error)?;
        Ok(DatabaseOpenResult {
            pool: Self { inner: pool },
            encryption_status: DatabaseEncryptionStatus::Encrypted,
        })
    }

    /// Acquire one connection synchronously. Only callable from
    /// `spawn_blocking` contexts.
    pub fn blocking_acquire(
        &self,
    ) -> std::result::Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, DbError>
    {
        self.inner.get().map_err(DbError::from)
    }

    /// Direct access to the r2d2 pool (escape hatch).
    pub fn raw(&self) -> &r2d2::Pool<r2d2_sqlite::SqliteConnectionManager> {
        &self.inner
    }
}

#[cfg(all(feature = "rusqlite", any(test, feature = "sqlcipher")))]
fn inspect_database_header(path: &Path) -> Result<DatabaseHeaderState> {
    use std::io::Read;

    if !path.exists() {
        return Ok(DatabaseHeaderState::Missing);
    }
    let metadata = std::fs::metadata(path).map_err(|e| MukeiError::Io(e.to_string()))?;
    if metadata.len() == 0 {
        return Ok(DatabaseHeaderState::Empty);
    }

    let mut file = std::fs::File::open(path).map_err(|e| MukeiError::Io(e.to_string()))?;
    let mut header = [0_u8; 16];
    let n = file
        .read(&mut header)
        .map_err(|e| MukeiError::Io(e.to_string()))?;
    if n < SQLITE_PLAIN_HEADER.len() {
        return Ok(DatabaseHeaderState::NotPlainSqlite);
    }
    if &header == SQLITE_PLAIN_HEADER {
        Ok(DatabaseHeaderState::PlainSqlite)
    } else {
        Ok(DatabaseHeaderState::NotPlainSqlite)
    }
}

#[cfg(feature = "sqlcipher")]
fn ensure_sqlcipher_available(c: &mut Conn) -> std::result::Result<(), rusqlite::Error> {
    let version = c.query_row("PRAGMA cipher_version", [], |row| row.get::<_, String>(0));
    match version {
        Ok(value) if !value.trim().is_empty() => Ok(()),
        Ok(_) | Err(rusqlite::Error::QueryReturnedNoRows) => Err(
            rusqlite::Error::InvalidParameterName("sqlcipher unavailable".to_string()),
        ),
        Err(err) => Err(err),
    }
}

#[cfg(feature = "sqlcipher")]
fn verify_keyed_database(c: &mut Conn) -> std::result::Result<(), rusqlite::Error> {
    c.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
}

#[cfg(feature = "sqlcipher")]
fn map_sqlcipher_pool_error(error: r2d2::Error) -> MukeiError {
    let message = error.to_string().to_ascii_lowercase();
    if message.contains("sqlcipher unavailable") || message.contains("no such pragma") {
        MukeiError::DatabaseEncryptionUnavailable
    } else if message.contains("file is not a database")
        || message.contains("not a database")
        || message.contains("file is encrypted")
    {
        MukeiError::DatabaseEncryptionInvalidKey
    } else if message.contains("database disk image is malformed")
        || message.contains("malformed")
        || message.contains("corrupt")
    {
        MukeiError::DatabaseEncryptionCorrupted
    } else {
        MukeiError::DatabaseInitFailed(format!("pool build: {error}"))
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

    #[test]
    fn header_detection_distinguishes_plain_sqlite_from_ciphertext() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.db");
        assert_eq!(
            inspect_database_header(&missing).unwrap(),
            DatabaseHeaderState::Missing
        );

        let empty = dir.path().join("empty.db");
        std::fs::write(&empty, []).unwrap();
        assert_eq!(
            inspect_database_header(&empty).unwrap(),
            DatabaseHeaderState::Empty
        );

        let plain = dir.path().join("plain.db");
        std::fs::write(&plain, SQLITE_PLAIN_HEADER).unwrap();
        assert_eq!(
            inspect_database_header(&plain).unwrap(),
            DatabaseHeaderState::PlainSqlite
        );

        let encrypted_like = dir.path().join("encrypted.db");
        std::fs::write(&encrypted_like, [0xA5_u8; 32]).unwrap();
        assert_eq!(
            inspect_database_header(&encrypted_like).unwrap(),
            DatabaseHeaderState::NotPlainSqlite
        );
    }

    #[cfg(feature = "sqlcipher")]
    #[test]
    fn sqlcipher_open_refuses_plain_sqlite_header_as_migration_required() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain.db");
        std::fs::write(&plain, SQLITE_PLAIN_HEADER).unwrap();

        let err = match DatabasePool::open_with_cipher_key_result(
            &plain,
            zeroize::Zeroizing::new(vec![7_u8; 32]),
        ) {
            Ok(_) => panic!("plain SQLite header must not open as encrypted DB"),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            MukeiError::DatabaseEncryptionMigrationRequired
        ));
    }

    #[cfg(feature = "sqlcipher")]
    #[test]
    fn sqlcipher_open_creates_non_plain_database_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("encrypted.db");

        let opened = DatabasePool::open_with_cipher_key_result(
            &path,
            zeroize::Zeroizing::new(vec![9_u8; 32]),
        )
        .unwrap();
        assert_eq!(
            opened.encryption_status,
            DatabaseEncryptionStatus::Encrypted
        );

        {
            let conn = opened.pool.blocking_acquire().unwrap();
            conn.execute_batch("CREATE TABLE encrypted_probe (id INTEGER PRIMARY KEY);")
                .unwrap();
        }

        assert_eq!(
            inspect_database_header(&path).unwrap(),
            DatabaseHeaderState::NotPlainSqlite
        );
    }
}
