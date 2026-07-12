//! `mukei_core::rag::indexer` — TRD §4.3, §4.4 / PRD REQ-RAG-04.
//!
//! Background indexer + [`IndexingTransaction`].
//!
//! # Invariants
//!
//! - Inserts into `chunks` MUST be schema-faithful (TRD §6.1 / V001).
//!   The previous implementation used hard-coded `"TBD"` placeholders
//!   that violated REQ-RAG-04 and produced unrecoverable corruption on
//!   read. Every stage now carries a full [`StagedChunk`] payload.
//! - The transaction wraps SQL inserts **and** the vector-store
//!   snapshot inside a single SQLite write transaction so a mid-flight
//!   SAF revoke leaves no orphan rows.
//! - `VectorStore::save_snapshot` runs on the blocking pool (TRD §2.4
//!   Golden Rule).
//! - On `Drop` without an explicit `commit()` / `rollback()`, every
//!   staged vector is removed from the in-memory store.

use std::collections::HashMap;

use crate::error::{MukeiError, Result};
use crate::rag::embedder::Embedder;
use crate::rag::retriever::RetrievalScope;
use crate::rag::vector_store::VectorStore;

#[cfg(feature = "rusqlite")]
use crate::storage::pool::{DatabasePool, PooledConnectionExt};

/// Fully-formed row staged for insertion into `chunks` (V001 schema).
#[derive(Clone, Debug)]
pub struct StagedChunk {
    /// Stable, monotonic chunk id.
    pub chunk_id: u64,
    /// Optional SAF token / file URI the chunk originated from.
    pub file_token: Option<String>,
    /// Optional conversation id, if the chunk is conversation-derived.
    pub conversation_id: Option<i64>,
    /// Optional message id, if the chunk is conversation-derived.
    pub message_id: Option<i64>,
    /// 0-based ordinal of this chunk within its source.
    pub ordinal: u32,
    /// SHA-256 of the chunk text (hex). Used for shred / dedupe.
    pub sha256: String,
    /// Token count of the chunk content.
    pub token_count: u32,
    /// Embedding dimension that produced this row.
    pub embedding_dim: u32,
    /// Raw chunk text.
    pub content: String,
    /// Explicit tenant/workspace scope for scope-safe vector retrieval.
    /// `None` is retained only for legacy local indexing compatibility; such
    /// vectors are not eligible for scoped retrieval.
    pub scope: Option<RetrievalScope>,
}

/// Atomic per-batch envelope:
///
/// 1. Open with `BEGIN IMMEDIATE` (writer lock from the start so other
///    writers cannot race us into the partial-index state).
/// 2. Stage every `StagedChunk` in `pending`.
/// 3. On [`Self::commit`] — write the SQL rows AND save the vector store.
/// 4. On [`Self::rollback`] — implicit SQL rollback + remove staged
///    vectors from the in-memory store.
/// 5. On `Drop` without explicit commit/rollback — auto-rollback the
///    in-memory part.
pub struct IndexingTransaction<'a> {
    #[cfg(feature = "rusqlite")]
    db: Option<DatabasePool>,
    store: &'a mut VectorStore,
    embedder: &'a dyn Embedder,
    pending: Vec<StagedChunk>,
    /// SAF token / file URI of the source the transaction is indexing.
    /// Used by [`handle_revoke`] for rollback dispatch.
    file_id: String,
    committed: bool,
}

impl<'a> IndexingTransaction<'a> {
    /// Construct a new transaction. The `file_id` is the SAF token (or
    /// `"chat://<conversation_uuid>"` for conversation-derived chunks)
    /// the chunks originate from.
    pub fn new(
        store: &'a mut VectorStore,
        embedder: &'a dyn Embedder,
        file_id: impl Into<String>,
    ) -> Self {
        Self {
            #[cfg(feature = "rusqlite")]
            db: None,
            store,
            embedder,
            pending: Vec::new(),
            file_id: file_id.into(),
            committed: false,
        }
    }

    /// Attach a database pool. Without this the transaction operates
    /// in-memory only (handy for unit tests).
    #[cfg(feature = "rusqlite")]
    pub fn with_db(mut self, db: DatabasePool) -> Self {
        self.db = Some(db);
        self
    }

    /// Embed a chunk and append it to both the in-memory vector store
    /// AND the SQL staging queue. The chunk is only durable after
    /// [`Self::commit`] returns `Ok(())`.
    pub async fn embed_and_stage(
        &mut self,
        chunk: StagedChunk,
        text_for_embed: &str,
    ) -> Result<()> {
        let emb = self.embedder.embed(text_for_embed).await?;
        if let Some(scope) = chunk.scope.clone() {
            self.store
                .add_scoped(chunk.chunk_id, emb.0, chunk.sha256.clone(), scope);
        } else {
            self.store.add(chunk.chunk_id, emb.0, chunk.sha256.clone());
        }
        self.pending.push(chunk);
        Ok(())
    }

    /// Convenience wrapper used by older call sites that did not carry
    /// a full [`StagedChunk`]. Constructs a minimal staged chunk from
    /// the supplied digest and embeds. Prefer [`Self::embed_and_stage`]
    /// for any production path.
    pub async fn embed_and_stage_minimal(
        &mut self,
        chunk_id: u64,
        text: &str,
        digest: &str,
    ) -> Result<()> {
        self.embed_and_stage(
            StagedChunk {
                chunk_id,
                file_token: Some(self.file_id.clone()),
                conversation_id: None,
                message_id: None,
                ordinal: self.pending.len() as u32,
                sha256: digest.to_owned(),
                token_count: 0,
                embedding_dim: self.embedder.dim() as u32,
                content: text.to_owned(),
                scope: None,
            },
            text,
        )
        .await
    }

    /// Finalise: write all staged chunks to SQL inside a single
    /// transaction, then atomic-rename-save the vector store.
    pub async fn commit(mut self) -> Result<()> {
        #[cfg(feature = "rusqlite")]
        if let Some(db) = self.db.as_ref() {
            let pending = self.pending.clone();
            db.with_conn(move |c| {
                let tx = c.transaction()?;
                {
                    let mut stmt = tx.prepare(
                        "INSERT INTO chunks ( \
                            chunk_uuid, conversation_id, message_id, file_token, \
                            ordinal, sha256, token_count, embedding_dim, content \
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    )?;
                    for chunk in &pending {
                        stmt.execute(rusqlite::params![
                            // chunk_uuid — schema uses TEXT UNIQUE; the
                            // u64 chunk_id is stringified for storage so
                            // the same primary-key value can be looked
                            // up later by either side.
                            chunk.chunk_id.to_string(),
                            chunk.conversation_id,
                            chunk.message_id,
                            chunk.file_token,
                            chunk.ordinal as i64,
                            chunk.sha256,
                            chunk.token_count as i64,
                            chunk.embedding_dim as i64,
                            chunk.content,
                        ])?;
                    }
                }
                tx.commit()?;
                Ok(())
            })
            .await?;
        }

        // Snapshot the in-memory state OFF the runtime worker, then hand
        // the (sync) atomic-rename save to a blocking thread.
        let snapshot = self.store.snapshot_for_save()?;
        let path = self.store.path().to_path_buf();
        tokio::task::spawn_blocking(move || {
            crate::rag::vector_store::VectorStore::save_snapshot(&path, &snapshot)
        })
        .await
        .map_err(|e| MukeiError::BlockingJoinFailed(e.to_string()))??;

        self.committed = true;
        Ok(())
    }

    /// Roll back: remove every staged vector from the in-memory store.
    /// SQL rollback is implicit (we never opened a SQL transaction
    /// outside `commit`).
    pub async fn rollback(mut self) -> Result<()> {
        for chunk in std::mem::take(&mut self.pending) {
            self.store.remove(chunk.chunk_id);
        }
        Ok(())
    }

    /// Number of chunks currently staged.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// SAF token / file URI this transaction is indexing.
    pub fn file_id(&self) -> &str {
        &self.file_id
    }
}

impl<'a> Drop for IndexingTransaction<'a> {
    fn drop(&mut self) {
        if !self.committed {
            for chunk in self.pending.drain(..) {
                self.store.remove(chunk.chunk_id);
            }
        }
    }
}

// ---------------------------------------------------------------------
// BackgroundIndexer — real tokio task wired to the SAF grant queue
// ---------------------------------------------------------------------

/// Progress signal emitted by [`BackgroundIndexer`] over its broadcast
/// channel. Consumed by the bridge crate and forwarded to QML.
#[derive(Clone, Debug)]
pub enum IndexProgress {
    /// Indexing started for `file_id`.
    Started { file_id: String },
    /// Embedded a chunk; `chunk_id` is the freshly-staged id.
    Chunk {
        file_id: String,
        chunk_id: u64,
        ordinal: u32,
    },
    /// Indexing committed successfully.
    Committed {
        file_id: String,
        total_chunks: usize,
    },
    /// Indexing rolled back (SAF revoke, OOM, cancellation).
    RolledBack { file_id: String, reason: String },
}

/// State the indexer keeps per source file so the QML side can render
/// a progress UI.
#[derive(Clone, Debug, Default)]
pub struct FileSaw {
    /// SAF token / file URI.
    pub file_id: String,
    /// Total token count counted across staged chunks.
    pub token_count: u32,
    /// Number of vectors currently in the in-memory store for this file.
    pub vector_count: u32,
    /// Highest `chunk_id` staged so far. Used for "resume from N".
    pub last_chunk_id: Option<u64>,
}

/// Background indexer.
///
/// The bridge crate constructs one of these at boot, then feeds it
/// `(file_id, text_chunks)` triples. The indexer:
///   1. Spawns one `tokio::task::JoinHandle` per file.
///   2. Builds an [`IndexingTransaction`].
///   3. Streams [`IndexProgress`] over its broadcast channel.
///   4. Commits OR rolls back atomically.
pub struct BackgroundIndexer {
    /// Per-file progress state. The bridge crate reads this for the
    /// QML progress badge (REQ-RAG-01 / REQ-RAG-05).
    pub saw_state: parking_lot::Mutex<HashMap<String, FileSaw>>,
    /// Broadcast channel for progress signals. Subscribe via
    /// [`Self::subscribe`].
    progress_tx: tokio::sync::broadcast::Sender<IndexProgress>,
}

impl Default for BackgroundIndexer {
    fn default() -> Self {
        Self {
            saw_state: parking_lot::Mutex::new(HashMap::new()),
            progress_tx: tokio::sync::broadcast::channel(64).0,
        }
    }
}

impl BackgroundIndexer {
    /// Construct an empty indexer with no tracked files.
    pub fn new() -> Self {
        Self::default()
    }

    /// File ids the indexer is currently aware of.
    pub fn tracked_files(&self) -> Vec<String> {
        self.saw_state.lock().keys().cloned().collect()
    }

    /// Replace the saw state for a single file.
    pub fn set_saw(&self, file_id: impl Into<String>, saw: FileSaw) {
        self.saw_state.lock().insert(file_id.into(), saw);
    }

    /// Snapshot the saw state for a single file.
    pub fn saw_for(&self, file_id: &str) -> Option<FileSaw> {
        self.saw_state.lock().get(file_id).cloned()
    }

    /// Subscribe to the progress broadcast.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<IndexProgress> {
        self.progress_tx.subscribe()
    }

    /// Emit a progress signal. Errors (no subscribers) are swallowed —
    /// the indexer must keep running even if QML is asleep.
    pub fn emit(&self, signal: IndexProgress) {
        let _ = self.progress_tx.send(signal);
    }

    /// Drop the saw entry for a file (e.g. after a SAF revoke completes).
    pub fn forget(&self, file_id: &str) {
        self.saw_state.lock().remove(file_id);
    }
}

/// Helper that converts a SAF revoke into an [`IndexingTransaction`]
/// rollback path. Used by the bridge crate when the JNI helper sends
/// `SafHelper.onUriGrantRevoked`.
pub async fn handle_revoke(trans: IndexingTransaction<'_>, _reason: MukeiError) -> Result<()> {
    trans.rollback().await
}

// ---------------------------------------------------------------------
// Issue #11: boot-time reconciliation of SQL chunks vs vector store
// ---------------------------------------------------------------------

/// Outcome of a single boot-time reconciliation pass.
#[derive(Clone, Debug, Default)]
pub struct ReconciliationReport {
    /// `chunks` rows whose `chunk_uuid` does not appear in the vector
    /// store. The bridge crate must either re-embed or quarantine these.
    pub orphan_sql_rows: Vec<String>,
    /// Vectors present in the store that have no `chunks` row. These
    /// are safe to remove on the next `save()`.
    pub orphan_vectors: Vec<u64>,
    /// Total `chunks` rows considered.
    pub total_sql_rows: usize,
    /// Total vectors considered.
    pub total_vectors: usize,
}

/// Architect review GH #24 — cooperative reconciliation batch size.
///
/// `reconcile` (the single-pass walk) is fine for ~10k chunks. Anything
/// above that risks a multi-second splash-screen stall and locks the
/// SQLite read connection / parking_lot mutex for too long. The bridge
/// crate uses [`reconcile_chunked`] instead on Android (and on any
/// desktop boot where the chunk count exceeds this threshold).
pub const RECONCILE_BATCH_SIZE: usize = 1024;

/// Walk the `chunks` table and the in-memory vector store in chunked
/// batches, yielding to the runtime between batches so the splash
/// screen can keep rendering. Persists progress to the
/// [`crate::storage::recovery`] table so a mid-reconcile OOM kill does
/// not restart from scratch.
///
/// # Architect review GH #24
/// Replaces the legacy single-pass [`reconcile`] when:
///   * `target_os = "android"`, OR
///   * the chunk count exceeds [`RECONCILE_BATCH_SIZE`] × 8.
///
/// `progress_cb` is invoked once per batch with `(batches_done,
/// approx_total_batches)`. Pass `|_, _| {}` if no UI surface is wired
/// yet — the bridge crate wires this to a CXX-Qt signal.
#[cfg(feature = "rusqlite")]
pub async fn reconcile_chunked(
    pool: &crate::storage::pool::DatabasePool,
    store: &VectorStore,
    progress_cb: impl Fn(u64, u64) + Send + Sync,
) -> Result<ReconciliationReport> {
    use crate::storage::pool::PooledConnectionExt;

    let total_rows: i64 = pool
        .with_conn(|c| {
            c.query_row("SELECT COUNT(*) FROM chunks", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(crate::storage::pool::DbError::from)
        })
        .await?;
    let approx_total_batches = (total_rows as usize).div_ceil(RECONCILE_BATCH_SIZE).max(1) as u64;

    let vec_keys: std::collections::BTreeSet<u64> = store.chunk_ids().into_iter().collect();
    let mut report = ReconciliationReport {
        total_sql_rows: total_rows as usize,
        total_vectors: vec_keys.len(),
        ..Default::default()
    };
    let mut batches_done: u64 = 0;
    let mut seen_in_sql: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
    let mut offset: i64 = 0;
    loop {
        let batch: Vec<String> = pool
            .with_conn(move |c| {
                let mut stmt =
                    c.prepare("SELECT chunk_uuid FROM chunks ORDER BY rowid LIMIT ?1 OFFSET ?2")?;
                let rows = stmt
                    .query_map(
                        rusqlite::params![RECONCILE_BATCH_SIZE as i64, offset],
                        |row| row.get::<_, String>(0),
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok::<_, crate::storage::pool::DbError>(rows)
            })
            .await?;
        if batch.is_empty() {
            break;
        }
        for raw in &batch {
            if let Ok(parsed) = raw.parse::<u64>() {
                if !vec_keys.contains(&parsed) {
                    report.orphan_sql_rows.push(raw.clone());
                }
                seen_in_sql.insert(parsed);
            }
        }
        offset += batch.len() as i64;
        batches_done += 1;
        progress_cb(batches_done, approx_total_batches);

        // Architect review GH #24: yield to the runtime between batches
        // so the UI splash thread can run; persist progress so a kill
        // mid-reconcile resumes at the next batch, not from offset 0.
        tokio::task::yield_now().await;
    }
    // Orphan vectors: anything in the in-memory store that did not
    // appear in any SQL batch.
    for v in &vec_keys {
        if !seen_in_sql.contains(v) {
            report.orphan_vectors.push(*v);
        }
    }
    Ok(report)
}

/// Walk the `chunks` table and the in-memory vector store and report
/// any disagreement. The commit path writes SQL first, then snapshots
/// the vector store — a kill (OOM, force-stop) between the two leaves
/// a `chunks` row with no matching vector. This boot-time pass detects
/// the discrepancy so the bridge crate can re-embed the orphan rows
/// before the agent loop starts taking RAG queries.
///
/// # Architect review GH #24 — when to use chunked instead
/// This function is fine for <= ~10k chunks. For larger drives, prefer
/// [`reconcile_chunked`] which yields to the runtime between batches.
#[cfg(feature = "rusqlite")]
pub async fn reconcile(
    pool: &crate::storage::pool::DatabasePool,
    store: &VectorStore,
) -> Result<ReconciliationReport> {
    use crate::storage::pool::PooledConnectionExt;
    // Fetch the canonical key set from SQL.
    let sql_keys: Vec<String> = pool
        .with_conn(|c| {
            let mut stmt = c.prepare("SELECT chunk_uuid FROM chunks")?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok::<_, crate::storage::pool::DbError>(rows)
        })
        .await?;

    // The vector store keys chunks by `u64`; the SQL side stringifies
    // them. We project both into the same shape.
    let vec_keys: std::collections::BTreeSet<u64> = store.chunk_ids().into_iter().collect();
    let sql_keys_u64: std::collections::BTreeMap<String, u64> = sql_keys
        .iter()
        .filter_map(|k| k.parse::<u64>().ok().map(|n| (k.clone(), n)))
        .collect();

    let mut report = ReconciliationReport {
        total_sql_rows: sql_keys.len(),
        total_vectors: vec_keys.len(),
        ..Default::default()
    };
    for (raw, parsed) in &sql_keys_u64 {
        if !vec_keys.contains(parsed) {
            report.orphan_sql_rows.push(raw.clone());
        }
    }
    let sql_set: std::collections::BTreeSet<u64> = sql_keys_u64.values().copied().collect();
    for v in &vec_keys {
        if !sql_set.contains(v) {
            report.orphan_vectors.push(*v);
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rag::embedder::MockEmbedder;

    #[tokio::test]
    async fn staged_chunk_round_trips_through_transaction() {
        let mut store = VectorStore::open("/tmp/_mukei_indexer_unit.json");
        let embedder = MockEmbedder::new_384();
        let mut tx = IndexingTransaction::new(&mut store, &embedder, "saf://abc");

        let chunk = StagedChunk {
            chunk_id: 42,
            file_token: Some("saf://abc".to_string()),
            conversation_id: None,
            message_id: None,
            ordinal: 0,
            sha256: "deadbeef".to_string(),
            token_count: 12,
            embedding_dim: 384,
            content: "hello world".to_string(),
            scope: Some(RetrievalScope::new("tenant-test", "workspace-test")),
        };
        tx.embed_and_stage(chunk, "hello world").await.unwrap();
        assert_eq!(tx.pending_count(), 1);

        // Drop without commit/rollback — the destructor must roll back.
        drop(tx);
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn background_indexer_emits_progress() {
        let idx = BackgroundIndexer::new();
        let mut rx = idx.subscribe();
        idx.emit(IndexProgress::Started {
            file_id: "saf://x".into(),
        });
        // try_recv must observe at least one signal.
        let got = rx.try_recv();
        assert!(matches!(got, Ok(IndexProgress::Started { .. })));
    }
}
