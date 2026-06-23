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

    // ----- Issue #2: SAF grants must persist to disk -----------------
    //
    // The previous implementation kept grants only in this `HashMap` and
    // their lifetime ended at process kill. The bridge crate's
    // `upsert_grant` now calls `persist_upsert` after the in-memory
    // mirror is updated; on boot, `hydrate_from_pool` re-populates the
    // map. Both paths respect the TRD §2.4 Golden Rule by routing the
    // SQL through `DatabasePool::with_conn`.

    /// Read every non-revoked row from the `saf_tokens` table and seed
    /// the in-memory map. Called by the bridge's `initialize()` once
    /// the SQLCipher pool is open.
    pub async fn hydrate_from_pool(
        &self,
        pool: &super::pool::DatabasePool,
    ) -> Result<usize> {
        use super::pool::PooledConnectionExt;
        let rows: Vec<SafTokenRow> = pool
            .with_conn(|c| {
                let mut stmt = c.prepare(
                    "SELECT token_id, source, target, mime_type, revoked, created_at \
                     FROM saf_tokens WHERE revoked = 0",
                )?;
                let rows = stmt
                    .query_map([], |row| {
                        let created: String = row.get(5)?;
                        Ok(SafTokenRow {
                            token_id: row.get(0)?,
                            source: row.get(1)?,
                            target: row.get(2)?,
                            mime: row.get(3)?,
                            revoked: row.get::<_, i64>(4)? != 0,
                            created: chrono::DateTime::parse_from_rfc3339(&created)
                                .map(|d| d.with_timezone(&chrono::Utc))
                                .unwrap_or_else(|_| chrono::Utc::now()),
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok::<_, super::pool::DbError>(rows)
            })
            .await?;
        self.load_from_db(rows.clone())?;
        Ok(rows.len())
    }

    /// Write a grant row through to SQL. Idempotent (UPSERT) so the
    /// bridge crate can call this on every `upsert` regardless of
    /// prior state.
    pub async fn persist_upsert(
        &self,
        pool: &super::pool::DatabasePool,
        row: SafTokenRow,
    ) -> Result<()> {
        use super::pool::PooledConnectionExt;
        let to_write = row.clone();
        pool.with_conn(move |c| {
            c.execute(
                "INSERT INTO saf_tokens \
                   (token_id, source, user_facing_label, target, mime_type, \
                    persistable, revoked, created_at) \
                 VALUES (?1, ?2, ?2, ?3, ?4, 1, ?5, ?6) \
                 ON CONFLICT(token_id) DO UPDATE SET \
                   target        = excluded.target, \
                   mime_type     = excluded.mime_type, \
                   revoked       = excluded.revoked, \
                   last_used_at  = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                rusqlite::params![
                    to_write.token_id,
                    to_write.source,
                    to_write.target,
                    to_write.mime,
                    if to_write.revoked { 1_i64 } else { 0 },
                    to_write.created.to_rfc3339(),
                ],
            )?;
            Ok::<_, super::pool::DbError>(())
        })
        .await?;
        // Mirror in memory after a successful write — readers must not
        // see an in-memory state that disagrees with disk.
        self.upsert(row)?;
        Ok(())
    }

    /// Soft-delete a grant on disk + in memory.
    pub async fn persist_revoke(
        &self,
        pool: &super::pool::DatabasePool,
        token: &str,
        reason: &str,
    ) -> Result<()> {
        use super::pool::PooledConnectionExt;
        let token_owned = token.to_string();
        let reason_owned = reason.to_string();
        pool.with_conn(move |c| {
            c.execute(
                "UPDATE saf_tokens SET revoked = 1, revoke_reason = ?2 WHERE token_id = ?1",
                rusqlite::params![token_owned, reason_owned],
            )?;
            Ok::<_, super::pool::DbError>(())
        })
        .await?;
        self.revoke(token)?;
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
