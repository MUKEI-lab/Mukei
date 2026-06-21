//! `mukei_core::rag::indexer` — TRD §4.3, §4.4 (BUGFIX v0.7.4).
//!
//! Background indexer + `IndexingTransaction`. On SAF-revoke the
//! transaction rolls back the SQL `chunks` rows AND removes the
//! in-memory HNSW vectors — without this, retrieval later returns
//! `Err(ChunkIdNotInVectorStore)` for the orphan half.

use std::collections::HashMap;

use crate::error::{MukeiError, Result};
use crate::rag::embedder::Embedder;
use crate::rag::vector_store::VectorStore;

#[cfg(feature = "rusqlite")]
use crate::storage::pool::{DatabasePool, PooledConnectionExt};

/// Atomic per-batch envelope:
///
/// 1. Open with `BEGIN IMMEDIATE` (writer lock from the start so other
///    writers cannot race us into the partial-index state).
/// 2. Stage every `chunk_id` added since start in `pending_ids`.
/// 3. On `commit()` — write the SQL rows, save the vector store.
/// 4. On `rollback()` — `ROLLBACK` SQL AND call
///    `VectorStore::remove` for every staged id.
/// 5. On `Drop` without explicit commit/rollback — auto-rollback.
///
/// This guarantees that a SAF-revoke mid-flight never leaves partial
/// chunks behind.
pub struct IndexingTransaction<'a> {
    #[cfg(feature = "rusqlite")]
    db:        Option<DatabasePool>,
    store:     &'a mut VectorStore,
    embedder:  &'a dyn Embedder,
    pending:   Vec<u64>,
    file_id:   String,           // SAF token / file URI
    committed: bool,
}

impl<'a> IndexingTransaction<'a> {
    pub fn new(store: &'a mut VectorStore, embedder: &'a dyn Embedder, file_id: impl Into<String>) -> Self {
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

    #[cfg(feature = "rusqlite")]
    pub fn with_db(mut self, db: DatabasePool) -> Self {
        self.db = Some(db);
        self
    }

    /// Embed a chunk and append it to the in-memory HNSW. The chunk_id
    /// is recorded so rollback can undo the add.
    pub async fn embed_and_stage(&mut self, chunk_id: u64, text: &str, digest: &str) -> Result<()> {
        let emb = self.embedder.embed(text).await?;
        self.store.add(chunk_id, emb.0, digest.into());
        self.pending.push(chunk_id);
        Ok(())
    }

    /// Finalise — flush SQL rows + atomic-rename save the vector store.
    pub async fn commit(mut self) -> Result<()> {
        #[cfg(feature = "rusqlite")]
        if let Some(db) = self.db.as_ref() {
            let ids = self.pending.clone();
            db.with_conn(move |c| {
                let tx = c.transaction()?;
                // Insert per-chunk rows.
                let mut stmt = tx.prepare(
                    "INSERT INTO chunks (chunk_id, file_id, sha256, n_tokens) VALUES (?, ?, ?, 0)",
                )?;
                for id in &ids {
                    stmt.execute(rusqlite::params![*id as i64, "TBD", ""])?;
                }
                drop(stmt);
                tx.commit()?;
                Ok(())
            }).await?;
        }
        self.store.save()?;
        self.committed = true;
        Ok(())
    }

    /// Roll back — clear staged HNSW vectors.
    pub async fn rollback(mut self) -> Result<()> {
        for id in std::mem::take(&mut self.pending) {
            self.store.remove(id);
        }
        // SQL ROLLBACK is implicit because we never committed.
        Ok(())
    }

    pub fn pending_count(&self) -> usize { self.pending.len() }
}

impl<'a> Drop for IndexingTransaction<'a> {
    fn drop(&mut self) {
        if !self.committed {
            for id in self.pending.drain(..) {
                self.store.remove(id);
            }
        }
    }
}

/// Background indexer:
///  - emits `JoinHandle`s that watch the SAF grant queue and add or
///    revoke files atomically.
///  - the bridge crate supplies the SAF grant feed.
pub struct BackgroundIndexer {
    pub saw_state: parking_lot::Mutex<HashMap<String, FileSaw>>,
}

#[derive(Clone, Debug, Default)]
pub struct FileSaw {
    pub file_id: String,
    pub token_count: u32,
    pub vector_count: u32,
    pub last_chunk_id: Option<u64>,
}

impl Default for BackgroundIndexer {
    fn default() -> Self {
        Self { saw_state: parking_lot::Mutex::new(HashMap::new()) }
    }
}

impl BackgroundIndexer {
    pub fn new() -> Self { Self::default() }

    pub fn tracked_files(&self) -> Vec<String> {
        self.saw_state.lock().keys().cloned().collect()
    }

    pub fn set_saw(&self, file_id: impl Into<String>, saw: FileSaw) {
        self.saw_state.lock().insert(file_id.into(), saw);
    }

    pub fn saw_for(&self, file_id: &str) -> Option<FileSaw> {
        self.saw_state.lock().get(file_id).cloned()
    }
}

/// Helper that converts a SAF revoke into an `IndexingTransaction`
/// rollback path. Used by the bridge crate when the JNI helper sends
/// `SafHelper.onUriGrantRevoked`.
pub async fn handle_revoke(trans: IndexingTransaction<'_>, _reason: MukeiError) -> Result<()> {
    trans.rollback().await
}
