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
#[cfg(feature = "rusqlite")]
use rusqlite::OptionalExtension;

/// Row in `saf_tokens` table.
#[cfg(feature = "rusqlite")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafTokenRow {
    pub token_id: String,
    pub source: String,
    pub target: String,
    pub mime: String,
    pub revoked: bool,
    pub created: chrono::DateTime<chrono::Utc>,
}

/// Durable plan for removing vector rows after the SQL-side revoke has
/// committed. The plan is also persisted in `document_tombstone`, so a
/// crash between SQL deletion and vector-store save can be retried at boot.
#[cfg(feature = "rusqlite")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafCleanupPlan {
    pub file_token: String,
    pub chunk_ids: Vec<u64>,
}

/// Revocation tombstone whose mandatory audit row has not yet been linked.
#[cfg(feature = "rusqlite")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnauditedDocumentRevocation {
    pub file_token: String,
    pub reason: String,
    pub chunks_deleted: usize,
}

/// Privacy-safe document projection for QML. `document_id` is a stable
/// one-way fingerprint, never the raw SAF token or content URI.
#[cfg(feature = "rusqlite")]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DocumentProjection {
    pub document_id: String,
    pub label: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub chunk_count: u64,
    pub revoked: bool,
    pub cleanup_pending: bool,
    pub cleanup_attempts: u64,
    pub last_error: Option<String>,
    pub permission_state: String,
    pub ingestion_state: String,
    pub ingestion_progress_percent: u32,
    pub ingestion_retryable: bool,
    pub ingestion_error: Option<String>,
    pub updated_at: String,
}

#[cfg(feature = "rusqlite")]
fn document_id_for_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(token.as_bytes());
    format!(
        "doc-{}",
        digest[..12]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
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
    pub fn new() -> Self {
        Self::default()
    }

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
    pub async fn hydrate_from_pool(&self, pool: &super::pool::DatabasePool) -> Result<usize> {
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
            let tombstoned: i64 = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM document_tombstone WHERE file_token = ?1)",
                [&to_write.token_id],
                |row| row.get(0),
            )?;
            if tombstoned != 0 {
                return Err(super::pool::DbError::Domain(MukeiError::SafRevoked));
            }
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

    /// Soft-delete a grant on disk and stage durable vector cleanup.
    /// SQL chunks are removed transactionally, while their vector ids are
    /// retained in `document_tombstone.chunk_ids_json` until the vector
    /// store has been shredded and saved successfully.
    pub async fn persist_revoke(
        &self,
        pool: &super::pool::DatabasePool,
        token: &str,
        reason: &str,
    ) -> Result<SafCleanupPlan> {
        use super::pool::PooledConnectionExt;
        let token_owned = token.to_string();
        let reason_owned = reason.to_string();
        let plan = pool
            .with_conn(move |c| {
                let tx = c.transaction()?;
                let chunk_ids = {
                    let mut stmt = tx.prepare(
                        "SELECT chunk_uuid FROM chunks WHERE file_token = ?1 ORDER BY id",
                    )?;
                    let rows = stmt
                        .query_map([&token_owned], |row| row.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    rows.into_iter()
                        .filter_map(|id| id.parse::<u64>().ok())
                        .collect::<Vec<_>>()
                };
                let changed = tx.execute(
                    "UPDATE saf_tokens \
                     SET revoked = 1, revoke_reason = ?2 \
                     WHERE token_id = ?1",
                    rusqlite::params![&token_owned, &reason_owned],
                )?;
                if changed == 0 {
                    return Err(super::pool::DbError::Domain(MukeiError::SafRequired));
                }

                let now = chrono::Utc::now().to_rfc3339();
                let chunk_ids_json = serde_json::to_string(&chunk_ids).map_err(|error| {
                    super::pool::DbError::Domain(MukeiError::Invariant(format!(
                        "SAF cleanup plan serialization failed: {error}"
                    )))
                })?;
                tx.execute(
                    "INSERT INTO document_tombstone (\
                        file_token, revoked_at, reason, chunks_deleted, cleanup_pending, \
                        audited_event_id, chunk_ids_json, cleanup_attempts, last_error, updated_at\
                     ) VALUES (?1, ?2, ?3, ?4, 1, NULL, ?5, 0, NULL, ?2) \
                     ON CONFLICT(file_token) DO UPDATE SET \
                        revoked_at = excluded.revoked_at, \
                        reason = excluded.reason, \
                        chunks_deleted = excluded.chunks_deleted, \
                        cleanup_pending = 1, \
                        chunk_ids_json = excluded.chunk_ids_json, \
                        last_error = NULL, \
                        updated_at = excluded.updated_at",
                    rusqlite::params![
                        &token_owned,
                        &now,
                        &reason_owned,
                        chunk_ids.len() as i64,
                        &chunk_ids_json,
                    ],
                )?;
                tx.execute("DELETE FROM chunks WHERE file_token = ?1", [&token_owned])?;
                tx.execute(
                    "UPDATE document_ingestion_jobs SET state = 'cancelled', retryable = 0, \
                     last_error = NULL, updated_at = ?2 WHERE token_id = ?1",
                    rusqlite::params![&token_owned, &now],
                )?;
                tx.commit()?;
                Ok::<_, super::pool::DbError>(SafCleanupPlan {
                    file_token: token_owned,
                    chunk_ids,
                })
            })
            .await?;
        // The database is the source of truth. A missing in-memory row
        // (for example after a partial hydration failure) must not turn a
        // committed revoke into an apparent failure for the caller.
        if let Err(error) = self.revoke(token) {
            if !matches!(&error, MukeiError::SafRequired) {
                return Err(error);
            }
        }
        Ok(plan)
    }

    /// List cleanup plans left pending by a crash or vector-store failure.
    pub async fn pending_document_cleanups(
        pool: &super::pool::DatabasePool,
    ) -> Result<Vec<SafCleanupPlan>> {
        use super::pool::PooledConnectionExt;
        pool.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT file_token, chunk_ids_json \
                 FROM document_tombstone \
                 WHERE cleanup_pending = 1 \
                 ORDER BY revoked_at ASC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter()
                .map(|(file_token, chunk_ids_json)| {
                    let chunk_ids =
                        serde_json::from_str::<Vec<u64>>(&chunk_ids_json).map_err(|error| {
                            let token_fingerprint = {
                                use sha2::{Digest, Sha256};
                                let digest = Sha256::digest(file_token.as_bytes());
                                digest[..6]
                                    .iter()
                                    .map(|byte| format!("{byte:02x}"))
                                    .collect::<String>()
                            };
                            tracing::error!(
                                token_fingerprint = %token_fingerprint,
                                error = %error,
                                "invalid document tombstone chunk id payload"
                            );
                            super::pool::DbError::Domain(MukeiError::DatabaseCorruption)
                        })?;
                    Ok(SafCleanupPlan {
                        file_token,
                        chunk_ids,
                    })
                })
                .collect::<std::result::Result<Vec<_>, super::pool::DbError>>()
        })
        .await
    }

    /// Atomically persist a native document grant and its initial ingestion job.
    /// The in-memory registry is updated only after the SQL transaction commits.
    pub async fn persist_document_grant(
        &self,
        pool: &super::pool::DatabasePool,
        row: SafTokenRow,
        permission_state: &str,
    ) -> Result<String> {
        use super::pool::PooledConnectionExt;
        if !matches!(
            permission_state,
            "persisted" | "transient" | "not_required" | "failed" | "unknown"
        ) {
            return Err(MukeiError::Invariant(
                "invalid document permission state".into(),
            ));
        }
        let document_id = document_id_for_token(&row.token_id);
        let row_for_db = row.clone();
        let document_id_for_db = document_id.clone();
        let permission_state = permission_state.to_string();
        pool.with_conn(move |c| {
            let tx = c.transaction()?;
            let tombstoned: i64 = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM document_tombstone WHERE file_token = ?1)",
                [&row_for_db.token_id],
                |row| row.get(0),
            )?;
            if tombstoned != 0 {
                return Err(super::pool::DbError::Domain(MukeiError::SafRevoked));
            }
            let now = chrono::Utc::now().to_rfc3339();
            tx.execute(
                "INSERT INTO saf_tokens                    (token_id, source, user_facing_label, target, mime_type,                     persistable, revoked, created_at, last_used_at, os_permission_state)                  VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)                  ON CONFLICT(token_id) DO UPDATE SET                    source = excluded.source,                    user_facing_label = excluded.user_facing_label,                    target = excluded.target,                    mime_type = excluded.mime_type,                    persistable = excluded.persistable,                    revoked = excluded.revoked,                    last_used_at = excluded.last_used_at,                    os_permission_state = excluded.os_permission_state",
                rusqlite::params![
                    row_for_db.token_id,
                    row_for_db.source,
                    row_for_db.target,
                    row_for_db.mime,
                    if permission_state == "persisted" { 1_i64 } else { 0_i64 },
                    if row_for_db.revoked { 1_i64 } else { 0_i64 },
                    row_for_db.created.to_rfc3339(),
                    now,
                    permission_state,
                ],
            )?;
            tx.execute(
                "INSERT INTO document_ingestion_jobs                  (document_id, token_id, state, progress_percent, chunk_count, retryable, last_error, created_at, updated_at)                  VALUES (?1, ?2, 'waiting_for_embedder', 0, 0, 1, NULL, ?3, ?3)                  ON CONFLICT(document_id) DO UPDATE SET                    token_id = excluded.token_id, state = 'waiting_for_embedder',                    progress_percent = 0, chunk_count = 0, retryable = 1,                    last_error = NULL, updated_at = excluded.updated_at",
                rusqlite::params![document_id_for_db, row_for_db.token_id, now],
            )?;
            tx.commit()?;
            Ok::<_, super::pool::DbError>(())
        })
        .await?;
        self.upsert(row)?;
        Ok(document_id)
    }

    /// Persist the actual OS permission outcome without exposing the URI to QML.
    pub async fn set_permission_state(
        pool: &super::pool::DatabasePool,
        token: &str,
        state: &str,
    ) -> Result<()> {
        use super::pool::PooledConnectionExt;
        let token = token.to_string();
        let state = state.to_string();
        pool.with_conn(move |c| {
            c.execute(
                "UPDATE saf_tokens \
                 SET os_permission_state = ?2, \
                     persistable = CASE WHEN ?2 = 'persisted' THEN 1 ELSE 0 END, \
                     last_used_at = ?3 \
                 WHERE token_id = ?1",
                rusqlite::params![token, state, chrono::Utc::now().to_rfc3339()],
            )?;
            Ok::<_, super::pool::DbError>(())
        })
        .await
    }

    /// Create or reset a durable ingestion job. Processing is performed by
    /// the production embedder worker; the UI never treats a queued row as indexed.
    pub async fn queue_document_ingestion(
        pool: &super::pool::DatabasePool,
        token: &str,
    ) -> Result<String> {
        use super::pool::PooledConnectionExt;
        let token = token.to_string();
        let document_id = document_id_for_token(&token);
        let id_for_db = document_id.clone();
        pool.with_conn(move |c| {
            let revoked: i64 = c.query_row(
                "SELECT revoked FROM saf_tokens WHERE token_id = ?1",
                [&token],
                |row| row.get(0),
            )?;
            if revoked != 0 {
                return Err(super::pool::DbError::Domain(MukeiError::SafRevoked));
            }
            let now = chrono::Utc::now().to_rfc3339();
            c.execute(
                "INSERT INTO document_ingestion_jobs \
                 (document_id, token_id, state, progress_percent, chunk_count, retryable, last_error, created_at, updated_at) \
                 VALUES (?1, ?2, 'waiting_for_embedder', 0, 0, 1, NULL, ?3, ?3) \
                 ON CONFLICT(document_id) DO UPDATE SET \
                   state = 'waiting_for_embedder', progress_percent = 0, retryable = 1, last_error = NULL, updated_at = excluded.updated_at",
                rusqlite::params![id_for_db, token, now],
            )?;
            Ok::<_, super::pool::DbError>(())
        })
        .await?;
        Ok(document_id)
    }

    /// Resolve a privacy-safe document id to its internal token for bridge-only operations.
    pub async fn token_for_document_id(
        pool: &super::pool::DatabasePool,
        document_id: &str,
    ) -> Result<String> {
        use super::pool::PooledConnectionExt;
        let document_id = document_id.to_string();
        pool.with_conn(move |c| {
            let direct: Option<String> = c
                .query_row(
                    "SELECT token_id FROM document_ingestion_jobs WHERE document_id = ?1",
                    [&document_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(token) = direct {
                return Ok(token);
            }
            let mut stmt = c.prepare("SELECT token_id FROM saf_tokens")?;
            let tokens = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            tokens
                .into_iter()
                .find(|token| document_id_for_token(token) == document_id)
                .ok_or_else(|| super::pool::DbError::Domain(MukeiError::SafRequired))
        })
        .await
    }

    /// Resolve an internal token to its URI for native permission release.
    /// This value never crosses the bridge into QML.
    pub async fn target_for_token(
        pool: &super::pool::DatabasePool,
        token: &str,
    ) -> Result<String> {
        use super::pool::PooledConnectionExt;
        let token = token.to_string();
        pool.with_conn(move |c| {
            c.query_row(
                "SELECT target FROM saf_tokens WHERE token_id = ?1",
                [&token],
                |row| row.get(0),
            )
            .map_err(super::pool::DbError::from)
        })
        .await
    }

    /// List document/index projections without exposing raw SAF tokens or URIs.
    pub async fn list_document_projections(
        pool: &super::pool::DatabasePool,
        limit: usize,
    ) -> Result<Vec<DocumentProjection>> {
        use super::pool::PooledConnectionExt;
        let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
        pool.with_conn(move |c| {
            let mut stmt = c.prepare(
                "SELECT s.token_id, s.user_facing_label, s.mime_type, s.size_bytes, s.revoked, \
                        COUNT(ch.chunk_uuid) AS chunk_count, \
                        COALESCE(t.cleanup_pending, 0), COALESCE(t.cleanup_attempts, 0), \
                        t.last_error, s.os_permission_state, \
                        COALESCE(j.state, 'queued'), COALESCE(j.progress_percent, 0), \
                        COALESCE(j.retryable, 1), j.last_error, \
                        COALESCE(j.updated_at, t.updated_at, s.last_used_at, s.created_at) \
                 FROM saf_tokens s \
                 LEFT JOIN chunks ch ON ch.file_token = s.token_id \
                 LEFT JOIN document_tombstone t ON t.file_token = s.token_id \
                 LEFT JOIN document_ingestion_jobs j ON j.token_id = s.token_id \
                 GROUP BY s.token_id, s.user_facing_label, s.mime_type, s.size_bytes, s.revoked, \
                          t.cleanup_pending, t.cleanup_attempts, t.last_error, t.updated_at, \
                          s.os_permission_state, j.state, j.progress_percent, j.retryable, \
                          j.last_error, j.updated_at, s.last_used_at, s.created_at \
                 ORDER BY COALESCE(j.updated_at, t.updated_at, s.last_used_at, s.created_at) DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map([limit], |row| {
                    let token: String = row.get(0)?;
                    let size: i64 = row.get(3)?;
                    let chunks: i64 = row.get(5)?;
                    let attempts: i64 = row.get(7)?;
                    Ok(DocumentProjection {
                        document_id: document_id_for_token(&token),
                        label: row.get(1)?,
                        mime_type: row.get(2)?,
                        size_bytes: u64::try_from(size.max(0)).unwrap_or(u64::MAX),
                        chunk_count: u64::try_from(chunks.max(0)).unwrap_or(u64::MAX),
                        revoked: row.get::<_, i64>(4)? != 0,
                        cleanup_pending: row.get::<_, i64>(6)? != 0,
                        cleanup_attempts: u64::try_from(attempts.max(0)).unwrap_or(u64::MAX),
                        last_error: row.get(8)?,
                        permission_state: row.get(9)?,
                        ingestion_state: row.get(10)?,
                        ingestion_progress_percent: u32::try_from(row.get::<_, i64>(11)?.clamp(0, 100)).unwrap_or(0),
                        ingestion_retryable: row.get::<_, i64>(12)? != 0,
                        ingestion_error: row.get(13)?,
                        updated_at: row.get(14)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok::<_, super::pool::DbError>(rows)
        })
        .await
    }

    /// List committed revocations that still need their mandatory audit
    /// row. This closes the crash window between the DB-first revoke
    /// transaction and the append-only audit insert.
    pub async fn unaudited_document_revocations(
        pool: &super::pool::DatabasePool,
    ) -> Result<Vec<UnauditedDocumentRevocation>> {
        use super::pool::PooledConnectionExt;
        pool.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT file_token, reason, chunks_deleted                  FROM document_tombstone                  WHERE audited_event_id IS NULL                  ORDER BY revoked_at ASC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    let chunks: i64 = row.get(2)?;
                    Ok(UnauditedDocumentRevocation {
                        file_token: row.get(0)?,
                        reason: row.get(1)?,
                        chunks_deleted: usize::try_from(chunks.max(0)).unwrap_or(usize::MAX),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok::<_, super::pool::DbError>(rows)
        })
        .await
    }

    /// Link a document tombstone to the exact hash-chained audit row
    /// that recorded the revoke request.
    pub async fn link_document_audit_event(
        pool: &super::pool::DatabasePool,
        token: &str,
        audit_row_id: i64,
    ) -> Result<()> {
        use super::pool::PooledConnectionExt;
        let token = token.to_string();
        pool.with_conn(move |c| {
            c.execute(
                "UPDATE document_tombstone \
                 SET audited_event_id = ?2, updated_at = ?3 \
                 WHERE file_token = ?1",
                rusqlite::params![token, audit_row_id, chrono::Utc::now().to_rfc3339()],
            )?;
            Ok::<_, super::pool::DbError>(())
        })
        .await
    }

    /// Mark a vector cleanup as durable and complete.
    pub async fn mark_document_cleanup_complete(
        pool: &super::pool::DatabasePool,
        token: &str,
    ) -> Result<()> {
        use super::pool::PooledConnectionExt;
        let token = token.to_string();
        pool.with_conn(move |c| {
            c.execute(
                "UPDATE document_tombstone \
                 SET cleanup_pending = 0, cleanup_attempts = cleanup_attempts + 1, \
                     last_error = NULL, updated_at = ?2 \
                 WHERE file_token = ?1",
                rusqlite::params![token, chrono::Utc::now().to_rfc3339()],
            )?;
            Ok::<_, super::pool::DbError>(())
        })
        .await
    }

    /// Keep the tombstone pending and retain only a redacted failure for
    /// the next boot-time retry.
    pub async fn mark_document_cleanup_failed(
        pool: &super::pool::DatabasePool,
        token: &str,
        error: &MukeiError,
    ) -> Result<()> {
        use super::pool::PooledConnectionExt;
        let token = token.to_string();
        let redacted = crate::diagnostics::sanitize_error_message(error.to_string());
        pool.with_conn(move |c| {
            c.execute(
                "UPDATE document_tombstone \
                 SET cleanup_pending = 1, cleanup_attempts = cleanup_attempts + 1, \
                     last_error = ?2, updated_at = ?3 \
                 WHERE file_token = ?1",
                rusqlite::params![token, redacted, chrono::Utc::now().to_rfc3339()],
            )?;
            Ok::<_, super::pool::DbError>(())
        })
        .await
    }

    /// `SafRegistry::resolve` — opaque-token → URI string. Returns
    /// `Err(MukeiError::SafRevoked)` for revoked rows.
    pub fn resolve(&self, token: &str) -> Result<String> {
        let map = self.tokens.lock();
        let row = map.get(token).ok_or(MukeiError::SafRequired)?;
        if row.revoked {
            return Err(MukeiError::SafRevoked);
        }
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
    pub fn new() -> Self {
        Self::default()
    }
    pub fn count(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "rusqlite")]
    async fn migrated_pool() -> crate::storage::DatabasePool {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("saf.db");
        let pool = crate::storage::DatabasePool::open(&db_path).unwrap();
        crate::storage::Migrator::embedded()
            .apply_pending(&pool)
            .await
            .unwrap();
        std::mem::forget(dir);
        pool
    }

    #[cfg(feature = "rusqlite")]
    #[test]
    fn resolve_returns_target_uri() {
        let reg = SafRegistry::new();
        reg.upsert(SafTokenRow {
            token_id: "tok-1".into(),
            source: "android-saf".into(),
            target:
                "content://com.android.externalstorage.documents/tree/primary%3ADocuments%2FMukei"
                    .into(),
            mime: "inode/directory".into(),
            revoked: false,
            created: chrono::Utc::now(),
        })
        .unwrap();
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
        })
        .unwrap();
        reg.revoke("tok-2").unwrap();
        let err = reg.resolve("tok-2").unwrap_err();
        assert!(matches!(err, MukeiError::SafRevoked));
    }

    #[cfg(feature = "rusqlite")]
    #[tokio::test]
    async fn persist_revoke_is_db_first_and_deletes_indexed_chunks() {
        use crate::storage::pool::PooledConnectionExt;

        let pool = migrated_pool().await;
        let reg = SafRegistry::new();
        let row = SafTokenRow {
            token_id: "tok-db-first".into(),
            source: "android-saf".into(),
            target: "content://doc".into(),
            mime: "text/plain".into(),
            revoked: false,
            created: chrono::Utc::now(),
        };
        reg.persist_upsert(&pool, row.clone()).await.unwrap();
        pool.with_conn(|c| {
            c.execute(
                "INSERT INTO chunks (chunk_uuid, file_token, ordinal, sha256, content) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params!["11", "tok-db-first", 0_i64, "sha", "alpha"],
            )?;
            c.execute(
                "INSERT INTO chunks (chunk_uuid, file_token, ordinal, sha256, content) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params!["12", "tok-db-first", 1_i64, "sha", "beta"],
            )?;
            Ok::<_, crate::storage::pool::DbError>(())
        })
        .await
        .unwrap();

        let plan = reg
            .persist_revoke(&pool, "tok-db-first", "user_revoke")
            .await
            .unwrap();
        assert_eq!(plan.file_token, "tok-db-first");
        assert_eq!(plan.chunk_ids, vec![11, 12]);
        assert!(matches!(
            reg.resolve("tok-db-first").unwrap_err(),
            MukeiError::SafRevoked
        ));

        let (revoked, remaining_chunks, cleanup_pending): (i64, i64, i64) = pool
            .with_conn(|c| {
                let revoked = c.query_row(
                    "SELECT revoked FROM saf_tokens WHERE token_id = ?1",
                    ["tok-db-first"],
                    |row| row.get::<_, i64>(0),
                )?;
                let remaining_chunks = c.query_row(
                    "SELECT COUNT(*) FROM chunks WHERE file_token = ?1",
                    ["tok-db-first"],
                    |row| row.get::<_, i64>(0),
                )?;
                let cleanup_pending = c.query_row(
                    "SELECT cleanup_pending FROM document_tombstone WHERE file_token = ?1",
                    ["tok-db-first"],
                    |row| row.get::<_, i64>(0),
                )?;
                Ok::<_, crate::storage::pool::DbError>((revoked, remaining_chunks, cleanup_pending))
            })
            .await
            .unwrap();
        assert_eq!(revoked, 1);
        assert_eq!(remaining_chunks, 0);
        assert_eq!(cleanup_pending, 1);

        let pending = SafRegistry::pending_document_cleanups(&pool).await.unwrap();
        assert_eq!(pending, vec![plan]);
        SafRegistry::mark_document_cleanup_complete(&pool, "tok-db-first")
            .await
            .unwrap();
        assert!(SafRegistry::pending_document_cleanups(&pool)
            .await
            .unwrap()
            .is_empty());
    }



    #[cfg(feature = "rusqlite")]
    #[tokio::test]
    async fn document_permission_and_ingestion_projection_round_trip() {
        use crate::storage::pool::PooledConnectionExt;

        let pool = migrated_pool().await;
        let reg = SafRegistry::new();
        let token = "tok-ingestion";
        let target = "content://documents/example";
        let document_id = reg
            .persist_document_grant(
                &pool,
                SafTokenRow {
                    token_id: token.into(),
                    source: "example.txt".into(),
                    target: target.into(),
                    mime: "text/plain".into(),
                    revoked: false,
                    created: chrono::Utc::now(),
                },
                "persisted",
            )
            .await
            .unwrap();

        assert_eq!(
            SafRegistry::token_for_document_id(&pool, &document_id)
                .await
                .unwrap(),
            token
        );
        assert_eq!(
            SafRegistry::target_for_token(&pool, token).await.unwrap(),
            target
        );

        let rows = SafRegistry::list_document_projections(&pool, 10)
            .await
            .unwrap();
        let projection = rows
            .iter()
            .find(|row| row.document_id == document_id)
            .expect("document projection");
        assert_eq!(projection.permission_state, "persisted");
        assert_eq!(projection.ingestion_state, "waiting_for_embedder");
        assert_eq!(projection.ingestion_progress_percent, 0);
        assert!(projection.ingestion_retryable);
        assert!(projection.ingestion_error.is_none());

        let (persistable, state): (i64, String) = pool
            .with_conn(move |c| {
                c.query_row(
                    "SELECT persistable, os_permission_state FROM saf_tokens WHERE token_id = ?1",
                    [token],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(crate::storage::pool::DbError::from)
            })
            .await
            .unwrap();
        assert_eq!(persistable, 1);
        assert_eq!(state, "persisted");
    }

    #[cfg(feature = "rusqlite")]
    #[tokio::test]
    async fn invalid_permission_state_does_not_persist_partial_grant() {
        use crate::storage::pool::PooledConnectionExt;

        let pool = migrated_pool().await;
        let reg = SafRegistry::new();
        let result = reg
            .persist_document_grant(
                &pool,
                SafTokenRow {
                    token_id: "tok-invalid-permission".into(),
                    source: "invalid.txt".into(),
                    target: "content://documents/invalid".into(),
                    mime: "text/plain".into(),
                    revoked: false,
                    created: chrono::Utc::now(),
                },
                "unexpected",
            )
            .await;
        assert!(result.is_err());
        let count: i64 = pool
            .with_conn(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM saf_tokens WHERE token_id = ?1",
                    ["tok-invalid-permission"],
                    |row| row.get(0),
                )
                .map_err(crate::storage::pool::DbError::from)
            })
            .await
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(reg.count(), 0);
    }

    #[cfg(feature = "rusqlite")]
    #[tokio::test]
    async fn document_revoke_cancels_queued_ingestion_job() {
        use crate::storage::pool::PooledConnectionExt;

        let pool = migrated_pool().await;
        let reg = SafRegistry::new();
        let token = "tok-cancel-ingestion";
        reg.persist_upsert(
            &pool,
            SafTokenRow {
                token_id: token.into(),
                source: "cancel.txt".into(),
                target: "content://documents/cancel".into(),
                mime: "text/plain".into(),
                revoked: false,
                created: chrono::Utc::now(),
            },
        )
        .await
        .unwrap();
        SafRegistry::queue_document_ingestion(&pool, token)
            .await
            .unwrap();
        reg.persist_revoke(&pool, token, "user_revoke")
            .await
            .unwrap();

        let (state, retryable): (String, i64) = pool
            .with_conn(move |c| {
                c.query_row(
                    "SELECT state, retryable FROM document_ingestion_jobs WHERE token_id = ?1",
                    [token],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(crate::storage::pool::DbError::from)
            })
            .await
            .unwrap();
        assert_eq!(state, "cancelled");
        assert_eq!(retryable, 0);
    }

    #[cfg(feature = "rusqlite")]
    #[tokio::test]
    async fn persist_revoke_keeps_memory_unchanged_when_db_revoke_fails() {
        let pool = migrated_pool().await;
        let reg = SafRegistry::new();
        reg.upsert(SafTokenRow {
            token_id: "memory-only".into(),
            source: "android-saf".into(),
            target: "content://still-live".into(),
            mime: "text/plain".into(),
            revoked: false,
            created: chrono::Utc::now(),
        })
        .unwrap();

        assert!(reg
            .persist_revoke(&pool, "memory-only", "user_revoke")
            .await
            .is_err());
        assert_eq!(reg.resolve("memory-only").unwrap(), "content://still-live");
    }
}
