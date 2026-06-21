//! `mukei_core::storage::saf` — TRD §5.4.
//!
//! Persistent SAF (Storage Access Framework on Android) URI grant
//! registry. The Rust side never holds a *path* — only opaque, scoped
//! tokens issued by the OS. The TODO list of methods is fixed and
//! required to match the bridge-layer SafRegistry QObject surface.
//!
//! ```text
//! SafRegistry::{load_from_db, resolve, upsert, revoke}
//! ```

#[cfg(feature = "rusqlite")]
use crate::error::{MukeiError, Result};

/// Row in `saf_tokens` table.
#[cfg(feature = "rusqlite")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafTokenRow {
    pub token_id: String,
    pub source:   String,
    pub target:   String,
    pub mime:     String,
    pub revoked:  bool,
    pub created:  chrono::DateTime<chrono::Utc>,
}

/// In-memory mirror of the persistent SAF grant table. The bridge
/// holds the `Arc<Mutex<_>>` so it can answer `resolve` calls without
/// hitting SQLite.
#[derive(Default, Debug)]
pub struct SafRegistry {
    tokens: parking_lot::Mutex<std::collections::HashMap<String, SafTokenRow>>,
}

#[cfg(feature = "rusqlite")]
impl SafRegistry {
    pub fn new() -> Self { Self::default() }

    /// `SafRegistry::load_from_db` — populate from the `saf_tokens` rows
    /// surfaced via `DatabasePool::with_conn`. Caller passes the raw
    /// rows; we project them into the in-memory map.
    pub fn load_from_db(&self, rows: Vec<SafTokenRow>) -> Result<()> {
        let mut map = self.tokens.lock();
        map.clear();
        for row in rows {
            if row.token_id.is_empty() {
                return Err(MukeiError::Invariant("empty saf_tokens row".into()));
            }
            map.insert(row.token_id.clone(), row);
        }
        Ok(())
    }

    /// `SafRegistry::resolve` — opaque-token → URI string. Returns
    /// `Err(MukeiError::SafRevoked)` for revoked rows.
    pub fn resolve(&self, token: &str) -> Result<String> {
        let map = self.tokens.lock();
        let row = map.get(token).ok_or(MukeiError::SafRequired)?;
        if row.revoked { return Err(MukeiError::SafRevoked); }
        Ok(row.target.clone())
    }

    /// `SafRegistry::upsert` — insert or update a single grant. The
    /// bridge crate forwards this to a `saf_tokens` SQL upsert.
    pub fn upsert(&self, row: SafTokenRow) -> Result<()> {
        let mut map = self.tokens.lock();
        map.insert(row.token_id.clone(), row);
        Ok(())
    }

    /// `SafRegistry::revoke` — mark revoked (soft-delete). Hard-delete
    /// happens after the 7-day grace window per BS v1.2 §11.
    pub fn revoke(&self, token: &str) -> Result<()> {
        let mut map = self.tokens.lock();
        let row = map.get_mut(token).ok_or(MukeiError::SafRequired)?;
        row.revoked = true;
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.tokens.lock().len()
    }
}

#[cfg(not(feature = "rusqlite"))]
impl SafRegistry {
    pub fn new() -> Self { Self::default() }
    pub fn count(&self) -> usize { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "rusqlite")]
    #[test]
    fn resolve_returns_target_uri() {
        let reg = SafRegistry::new();
        reg.upsert(SafTokenRow {
            token_id: "tok-1".into(),
            source: "android-saf".into(),
            target: "content://com.android.externalstorage.documents/tree/primary%3ADocuments%2FMukei".into(),
            mime: "inode/directory".into(),
            revoked: false,
            created: chrono::Utc::now(),
        }).unwrap();
        assert_eq!(
            reg.resolve("tok-1").unwrap(),
            "content://com.android.externalstorage.documents/tree/primary%3ADocuments%2FMukei"
        );
    }

    #[cfg(feature = "rusqlite")]
    #[test]
    fn revoke_blocks_subsequent_resolve() {
        let reg = SafRegistry::new();
        reg.upsert(SafTokenRow {
            token_id: "tok-2".into(),
            source: "x".into(),
            target: "content://x".into(),
            mime: "*/*".into(),
            revoked: false,
            created: chrono::Utc::now(),
        }).unwrap();
        reg.revoke("tok-2").unwrap();
        let err = reg.resolve("tok-2").unwrap_err();
        assert!(matches!(err, MukeiError::SafRevoked));
    }
}
