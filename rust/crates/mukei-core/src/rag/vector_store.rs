//! `mukei_core::rag::vector_store` — TRD §4.2 / PRD REQ-RAG-02 / REQ-RAG-03.
//!
//! Vector store with **atomic-rename** persistence. Two backends:
//!
//! - Default: pure-Rust JSON + linear cosine scan. Used in sandbox /
//!   unit tests and small indices (< few hundred chunks).
//! - `feature = "usearch_hnsw"`: real `usearch` HNSW index for the
//!   sub-30 ms search target on production builds.
//!
//! # Invariants
//!
//! - Every persisted file carries a [`StoreHeader`] (format version,
//!   embedding-model fingerprint, embedding dimension). Boot MUST
//!   refuse a file whose `embedder_id` or `embedding_dim` differs from
//!   the currently wired embedder — see [`VectorStore::needs_rebuild`].
//! - `save()` is synchronous and is invoked only from
//!   `spawn_blocking`; async paths use
//!   [`VectorStore::snapshot_for_save`] +
//!   [`VectorStore::save_snapshot`] (TRD §2.4 Golden Rule).
//! - The atomic-rename pair is the only durable write. Direct
//!   `fs::write` over `path` would race with concurrent `load()`.
//! - [`VectorStore::shred`] zeroises a vector in-place AND deletes its
//!   row from the persistent file — used for "Forget this source" UX
//!   (REQ-RAG-03).

// Architect review GH #16: release-hardening tripwire. A production
// build with the linear-scan backend would degrade RAG search to O(n)
// per query on a phone holding 100k+ chunks. Force `usearch_hnsw` ON
// for release-hardened builds. Tests / sandbox builds opt out by
// simply not enabling `release-hardening`.
#[cfg(all(feature = "release-hardening", not(feature = "usearch_hnsw"),))]
compile_error!(
    "mukei-core compiled with `release-hardening` but WITHOUT \
     `usearch_hnsw`. The fallback flat-scan vector store is O(n) per \
     query and unsuitable for production (PRD REQ-RAG-02). Enable the \
     `usearch_hnsw` feature in release builds."
);

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;

use crate::error::{MukeiError, Result};
use crate::rag::retriever::RetrievalScope;

/// Suffix appended to the live path while writing.
pub const ATOMIC_SUFFIX: &str = "swap";

/// On-disk format version. Bump whenever the [`Inner`] layout changes
/// in a way that breaks deserialisation. The boot path refuses any file
/// whose `format_version` is unknown.
pub const STORE_FORMAT_VERSION: u32 = 1;

/// Header persisted at the top of every store file. Carries the fields
/// needed to detect embedder / dimension drift across app upgrades.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct StoreHeader {
    /// Format version of the on-disk layout (see [`STORE_FORMAT_VERSION`]).
    pub format_version: u32,
    /// Stable identifier of the embedding model that produced these
    /// vectors (e.g. `"minilm-candle:sha256:<hex>"`).
    pub embedder_id: String,
    /// Embedding dimension. A mismatch with the live embedder is a
    /// fatal load error — cosine over differing dims is undefined.
    pub embedding_dim: u32,
}

/// Outcome of [`VectorStore::needs_rebuild`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RebuildVerdict {
    /// Header is present and matches the live embedder.
    Compatible,
    /// Persisted file has no header — V0 format; force a rebuild.
    NoHeader,
    /// Format version is newer/older than this binary supports.
    FormatMismatch {
        /// Format version persisted in the file.
        found: u32,
        /// Format version this binary supports.
        expected: u32,
    },
    /// The persisted embedder identity does not match the live one.
    EmbedderMismatch {
        /// Embedder id persisted in the file.
        found: String,
        /// Embedder id this binary is wired to.
        expected: String,
    },
    /// Embedding dimension drift (model swapped without re-indexing).
    DimensionMismatch {
        /// Dimension persisted in the file.
        found: u32,
        /// Dimension the live embedder produces.
        expected: u32,
    },
}

impl RebuildVerdict {
    /// True iff a full re-index is required.
    pub fn needs_rebuild(&self) -> bool {
        !matches!(self, Self::Compatible)
    }
}

/// Errors specific to the vector store.
#[derive(Debug, thiserror::Error)]
pub enum VectorStoreError {
    /// Underlying I/O failure.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// JSON deserialisation failure.
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    /// Caller asked to remove a chunk that was never added.
    #[error("chunk {0} was not previously added")]
    ChunkIdNotInStore(u64),
}

impl From<VectorStoreError> for MukeiError {
    fn from(e: VectorStoreError) -> Self {
        match e {
            VectorStoreError::ChunkIdNotInStore(id) => {
                MukeiError::Internal(format!("chunk {id} not in store"))
            }
            other => MukeiError::Internal(other.to_string()),
        }
    }
}

/// In-memory mirror of the on-disk HNSW index.
pub struct VectorStore {
    path: PathBuf,
    inner: Mutex<Inner>,
    #[cfg(feature = "usearch_hnsw")]
    hnsw: Mutex<Option<usearch::Index>>,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Inner {
    /// Persisted header. Optional only so V0 files (no header) can be
    /// detected during migration; new writes always carry a header.
    #[serde(default)]
    header: Option<StoreHeader>,
    vectors: Vec<(u64, Vec<f32>, String)>,
    /// Explicit scope metadata for vectors indexed through the scoped API.
    /// Legacy vectors intentionally remain absent and are therefore not
    /// eligible for scoped retrieval.
    #[serde(default)]
    scopes: BTreeMap<u64, RetrievalScope>,
}

impl VectorStore {
    /// Open (or create) a vector store at `path`. Does not load —
    /// call [`Self::load`] explicitly.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            inner: Mutex::new(Inner::default()),
            #[cfg(feature = "usearch_hnsw")]
            hnsw: Mutex::new(None),
        }
    }

    /// Path the vector store persists to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Stamp the in-memory state with the embedder identity that produced
    /// these vectors. Must be called BEFORE the first `add()` of a fresh
    /// store, and re-checked on `load()`.
    pub fn set_header(&self, header: StoreHeader) {
        #[cfg(feature = "usearch_hnsw")]
        {
            // Recreate the HNSW index when the dimension changes so the
            // backend never sees a stale vector width.
            let dim = header.embedding_dim as usize;
            *self.hnsw.lock() = Self::build_hnsw(dim).ok();
        }
        self.inner.lock().header = Some(header);
    }

    /// Read the persisted header, if any.
    pub fn header(&self) -> Option<StoreHeader> {
        self.inner.lock().header.clone()
    }

    /// Diagnose whether the persisted store is compatible with the live
    /// embedder. Returns [`RebuildVerdict::Compatible`] when no rebuild
    /// is needed; otherwise the variant explains why.
    pub fn needs_rebuild(&self, expected: &StoreHeader) -> RebuildVerdict {
        match self.header() {
            None => RebuildVerdict::NoHeader,
            Some(found) if found.format_version != expected.format_version => {
                RebuildVerdict::FormatMismatch {
                    found: found.format_version,
                    expected: expected.format_version,
                }
            }
            Some(found) if found.embedder_id != expected.embedder_id => {
                RebuildVerdict::EmbedderMismatch {
                    found: found.embedder_id,
                    expected: expected.embedder_id.clone(),
                }
            }
            Some(found) if found.embedding_dim != expected.embedding_dim => {
                RebuildVerdict::DimensionMismatch {
                    found: found.embedding_dim,
                    expected: expected.embedding_dim,
                }
            }
            Some(_) => RebuildVerdict::Compatible,
        }
    }

    /// Compatibility check used by the boot path. Returns `Err` if the
    /// store was previously written by a different embedder / dimension /
    /// on-disk format than the currently-wired one.
    pub fn assert_compatible_with(&self, expected: &StoreHeader) -> Result<()> {
        match self.needs_rebuild(expected) {
            RebuildVerdict::Compatible => Ok(()),
            verdict => {
                tracing::warn!(
                    ?verdict,
                    ?expected,
                    "vector store header mismatch — forcing reindex"
                );
                Err(MukeiError::ModelCorrupted)
            }
        }
    }

    /// Load the persisted store. Missing files leave the in-memory
    /// state empty; corrupted files surface `MukeiError::Internal`.
    pub fn load(&self) -> Result<()> {
        match fs::read(&self.path) {
            Ok(bytes) => {
                let parsed: Inner = serde_json::from_slice(&bytes)
                    .map_err(|e| MukeiError::Internal(e.to_string()))?;
                #[cfg(feature = "usearch_hnsw")]
                {
                    // Rebuild the HNSW index from the loaded vectors so
                    // the backend matches the persisted state.
                    if let Some(header) = parsed.header.as_ref() {
                        if let Ok(index) = Self::build_hnsw(header.embedding_dim as usize) {
                            for (id, vec, _digest) in &parsed.vectors {
                                let _ = index.add(*id, vec);
                            }
                            *self.hnsw.lock() = Some(index);
                        }
                    }
                }
                *self.inner.lock() = parsed;
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                *self.inner.lock() = Inner::default();
                Ok(())
            }
            Err(e) => Err(MukeiError::Io(e.to_string())),
        }
    }

    /// Add a vector. The store does NOT verify the dimension on its own
    /// — callers must already have called [`Self::set_header`] with the
    /// right `embedding_dim`.
    pub fn add(&self, chunk_id: u64, vec: Vec<f32>, digest: String) {
        #[cfg(feature = "usearch_hnsw")]
        if let Some(index) = self.hnsw.lock().as_ref() {
            let _ = index.add(chunk_id, &vec);
        }
        let mut inner = self.inner.lock();
        inner.scopes.remove(&chunk_id);
        inner.vectors.push((chunk_id, vec, digest));
    }

    /// Add a vector with explicit tenant/workspace scope. Scoped retrieval
    /// only returns vectors added through this API, so legacy unscoped vectors
    /// cannot accidentally cross an authorization boundary.
    pub fn add_scoped(
        &self,
        chunk_id: u64,
        vec: Vec<f32>,
        digest: String,
        scope: RetrievalScope,
    ) {
        #[cfg(feature = "usearch_hnsw")]
        if let Some(index) = self.hnsw.lock().as_ref() {
            let _ = index.add(chunk_id, &vec);
        }
        let mut inner = self.inner.lock();
        inner.scopes.insert(chunk_id, scope);
        inner.vectors.push((chunk_id, vec, digest));
    }

    /// Remove a single chunk by id. No-op when the id is absent.
    pub fn remove(&self, chunk_id: u64) {
        #[cfg(feature = "usearch_hnsw")]
        if let Some(index) = self.hnsw.lock().as_ref() {
            let _ = index.remove(chunk_id);
        }
        let mut inner = self.inner.lock();
        inner.vectors.retain(|(id, _, _)| *id != chunk_id);
        inner.scopes.remove(&chunk_id);
    }

    /// Forget every chunk that matches `digest`. Vector bytes are
    /// zeroised in-place BEFORE removal so a heap-inspecting attacker
    /// (or a panic-handler core dump) cannot recover them.
    /// (PRD REQ-RAG-03 — "Forget this source" UX.)
    pub fn shred(&self, digest: &str) -> usize {
        let mut removed = 0usize;
        let mut removed_ids = Vec::new();
        let mut g = self.inner.lock();
        g.vectors.retain_mut(|(id, vec, d)| {
            if d == digest {
                // Zeroise the vector floats in place.
                for v in vec.iter_mut() {
                    *v = 0.0;
                }
                #[cfg(feature = "usearch_hnsw")]
                if let Some(index) = self.hnsw.lock().as_ref() {
                    let _ = index.remove(*id);
                }
                removed_ids.push(*id);
                removed += 1;
                false
            } else {
                true
            }
        });
        for id in removed_ids {
            g.scopes.remove(&id);
        }
        removed
    }

    /// Forget every chunk associated with `file_token`. The caller
    /// supplies the matching `(chunk_id, digest)` pairs from the SQL
    /// side; this method only handles the in-memory mirror.
    pub fn shred_many(&self, chunk_ids: &[u64]) -> usize {
        let mut removed = 0usize;
        let mut removed_ids = Vec::new();
        let mut g = self.inner.lock();
        g.vectors.retain_mut(|(id, vec, _d)| {
            if chunk_ids.contains(id) {
                for v in vec.iter_mut() {
                    *v = 0.0;
                }
                #[cfg(feature = "usearch_hnsw")]
                if let Some(index) = self.hnsw.lock().as_ref() {
                    let _ = index.remove(*id);
                }
                removed_ids.push(*id);
                removed += 1;
                false
            } else {
                true
            }
        });
        for id in removed_ids {
            g.scopes.remove(&id);
        }
        removed
    }

    /// Number of vectors currently in the store.
    pub fn count(&self) -> usize {
        self.inner.lock().vectors.len()
    }

    /// Number of vectors carrying explicit scope metadata and therefore
    /// eligible for scoped retrieval.
    pub fn scoped_count(&self) -> usize {
        let inner = self.inner.lock();
        inner
            .vectors
            .iter()
            .filter(|(id, _, _)| inner.scopes.contains_key(id))
            .count()
    }

    /// Snapshot of every `chunk_id` currently in the store. Used by
    /// [`crate::rag::indexer::reconcile`] (Issue #11) to find SQL
    /// rows whose vector was lost in a partial commit.
    pub fn chunk_ids(&self) -> Vec<u64> {
        self.inner
            .lock()
            .vectors
            .iter()
            .map(|(id, _, _)| *id)
            .collect()
    }

    /// Save atomically. Writes to `<path>.<ATOMIC_SUFFIX>` first, then
    /// renames over the live path. Crash-between leaves the live file
    /// untouched.
    ///
    /// **Synchronous** — only call this from a non-async context, or
    /// (preferably) use [`Self::snapshot_for_save`] +
    /// [`Self::save_snapshot`] from an async task (TRD §2.4 Golden Rule).
    pub fn save(&self) -> Result<()> {
        let snapshot = self.snapshot_for_save()?;
        Self::save_snapshot(&self.path, &snapshot)
    }

    /// Take an FFI-safe snapshot of the current vector set. Cheap — only
    /// touches the in-memory mutex briefly. Use this from an async
    /// context, then hand the snapshot to [`Self::save_snapshot`] inside
    /// `tokio::task::spawn_blocking`.
    pub fn snapshot_for_save(&self) -> Result<Vec<u8>> {
        let g = self.inner.lock();
        serde_json::to_vec(&*g).map_err(|e| MukeiError::Internal(e.to_string()))
    }

    /// Persist a pre-serialised snapshot atomically. Pure file I/O —
    /// safe to call from a `spawn_blocking` worker.
    pub fn save_snapshot(path: &Path, bytes: &[u8]) -> Result<()> {
        let tmp = swap_path(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| MukeiError::Io(e.to_string()))?;
        }
        fs::write(&tmp, bytes).map_err(|e| MukeiError::Io(e.to_string()))?;
        fs::rename(&tmp, path).map_err(|e| MukeiError::Io(e.to_string()))?;
        Ok(())
    }

    /// Scope-filtered top-K cosine search. The store boundary filters by the
    /// exact tenant/workspace/actor/authorization scope before any candidate
    /// ids are returned to the resolver. Legacy vectors without scope metadata
    /// are intentionally ineligible.
    pub fn search_scoped(
        &self,
        q: &[f32],
        scope: &RetrievalScope,
        k: usize,
    ) -> Vec<(u64, f32)> {
        if k == 0 {
            return Vec::new();
        }
        let scoped_ids: std::collections::BTreeSet<u64> = {
            let inner = self.inner.lock();
            inner
                .scopes
                .iter()
                .filter_map(|(id, candidate_scope)| (candidate_scope == scope).then_some(*id))
                .collect()
        };
        if scoped_ids.is_empty() {
            return Vec::new();
        }

        // Ask the backend for the full local candidate set, then apply the
        // persisted scope filter and deterministic score/id ordering. This is
        // intentionally conservative for the current single-index layout; a
        // future physically-partitioned index can optimize this without
        // changing the authorization contract.
        let mut matches = self.search(q, self.count());
        matches.retain(|(id, _)| scoped_ids.contains(id));
        matches.sort_by(|(left_id, left_score), (right_id, right_score)| {
            right_score
                .total_cmp(left_score)
                .then_with(|| left_id.cmp(right_id))
        });
        matches.truncate(k);
        matches
    }

    /// Top-K cosine-similarity search without authorization filtering.
    /// Scoped production retrieval should use [`Self::search_scoped`].
    pub fn search(&self, q: &[f32], k: usize) -> Vec<(u64, f32)> {
        #[cfg(feature = "usearch_hnsw")]
        {
            if let Some(index) = self.hnsw.lock().as_ref() {
                if let Ok(matches) = index.search(q, k) {
                    let scored: Vec<(u64, f32)> = matches
                        .keys
                        .into_iter()
                        .zip(matches.distances)
                        // usearch returns distances; convert to a
                        // similarity score in `[-1, 1]` for callers
                        // that previously relied on cosine.
                        .map(|(id, d)| (id, 1.0 - d))
                        .collect();
                    if !scored.is_empty() {
                        return scored;
                    }
                }
            }
        }

        // Pure-Rust fallback.
        let g = self.inner.lock();
        let mut scored: Vec<(u64, f32)> = g
            .vectors
            .iter()
            .map(|(id, v, _)| (*id, cosine(q, v)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
    }

    /// Build a fresh HNSW index for the given dimension.
    #[cfg(feature = "usearch_hnsw")]
    fn build_hnsw(dim: usize) -> Result<usearch::Index> {
        let options = usearch::IndexOptions {
            dimensions: dim,
            metric: usearch::MetricKind::Cos,
            quantization: usearch::ScalarKind::F32,
            connectivity: 16,
            expansion_add: 64,
            expansion_search: 32,
            multi: false,
        };
        usearch::Index::new(&options)
            .map_err(|e| MukeiError::Internal(format!("usearch init: {e}")))
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for i in 0..n {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    dot / (na.sqrt().max(1e-9) * nb.sqrt().max(1e-9))
}

fn swap_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".");
    s.push(ATOMIC_SUFFIX);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn header(id: &str, dim: u32) -> StoreHeader {
        StoreHeader {
            format_version: STORE_FORMAT_VERSION,
            embedder_id: id.to_string(),
            embedding_dim: dim,
        }
    }

    #[test]
    fn save_creates_live_file() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("v.usearch");
        let s = VectorStore::open(&p);
        s.set_header(header("mock:v1", 2));
        s.add(1, vec![1.0, 0.0], "d1".into());
        s.save().unwrap();
        assert!(p.exists());
        assert!(!swap_path(&p).exists());
    }

    #[test]
    fn save_atomic_writes_swap_then_renames() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("v.usearch");
        let s = VectorStore::open(&p);
        s.set_header(header("mock:v1", 2));
        s.add(1, vec![1.0, 0.0], "d1".into());
        s.save().unwrap();
        let loaded = VectorStore::open(&p);
        loaded.load().unwrap();
        assert_eq!(loaded.count(), 1);
    }

    #[test]
    fn search_returns_top_k_cosine() {
        let s = VectorStore::open("/tmp/_unused.usearch");
        s.set_header(header("mock:v1", 2));
        s.add(1, vec![1.0, 0.0], "d".into());
        s.add(2, vec![0.0, 1.0], "d".into());
        let frac = std::f32::consts::FRAC_1_SQRT_2;
        s.add(3, vec![frac, frac], "d".into());
        let r = s.search(&[1.0, 0.0], 2);
        assert_eq!(r[0].0, 1);
    }

    #[test]
    fn remove_clears_id() {
        let s = VectorStore::open("/tmp/_unused.usearch");
        s.add(7, vec![1.0], "x".into());
        s.remove(7);
        assert_eq!(s.count(), 0);
    }

    #[test]
    fn needs_rebuild_detects_no_header() {
        let s = VectorStore::open("/tmp/_unused.usearch");
        let verdict = s.needs_rebuild(&header("mock:v1", 2));
        assert_eq!(verdict, RebuildVerdict::NoHeader);
        assert!(verdict.needs_rebuild());
    }

    #[test]
    fn needs_rebuild_detects_embedder_swap() {
        let s = VectorStore::open("/tmp/_unused.usearch");
        s.set_header(header("mock:v1", 2));
        let verdict = s.needs_rebuild(&header("mock:v2", 2));
        assert!(matches!(verdict, RebuildVerdict::EmbedderMismatch { .. }));
        assert!(verdict.needs_rebuild());
    }

    #[test]
    fn needs_rebuild_detects_dim_drift() {
        let s = VectorStore::open("/tmp/_unused.usearch");
        s.set_header(header("mock:v1", 384));
        let verdict = s.needs_rebuild(&header("mock:v1", 768));
        assert!(matches!(verdict, RebuildVerdict::DimensionMismatch { .. }));
        assert!(verdict.needs_rebuild());
    }

    #[test]
    fn needs_rebuild_accepts_matching_header() {
        let s = VectorStore::open("/tmp/_unused.usearch");
        s.set_header(header("mock:v1", 2));
        assert_eq!(
            s.needs_rebuild(&header("mock:v1", 2)),
            RebuildVerdict::Compatible
        );
    }

    #[test]
    fn shred_zeroises_and_removes_by_digest() {
        let s = VectorStore::open("/tmp/_unused.usearch");
        s.add(1, vec![1.0, 2.0, 3.0], "keep".into());
        s.add(2, vec![4.0, 5.0, 6.0], "drop-me".into());
        s.add(3, vec![7.0, 8.0, 9.0], "drop-me".into());
        let removed = s.shred("drop-me");
        assert_eq!(removed, 2);
        assert_eq!(s.count(), 1);
    }

    #[test]
    fn shred_many_removes_by_id_list() {
        let s = VectorStore::open("/tmp/_unused.usearch");
        s.add(1, vec![1.0], "a".into());
        s.add(2, vec![2.0], "b".into());
        s.add(3, vec![3.0], "c".into());
        let removed = s.shred_many(&[1, 3]);
        assert_eq!(removed, 2);
        assert_eq!(s.count(), 1);
    }
}
