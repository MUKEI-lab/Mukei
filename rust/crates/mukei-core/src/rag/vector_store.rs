//! `mukei_core::rag::vector_store` — TRD §4.2.
//!
//! usearch HNSW wrapper with **atomic-rename** save semantics.
//! Every save writes to a sibling `path.<ATOMIC_SUFFIX>` first; on
//! success, `rename(path.<ATOMIC_SUFFIX>, path)` overwrites the live
//! file in a single syscall. Crash-between is recovered by the boot
//! path (§4.5 / §11.1).
//!
//! # Invariants
//!
//! - Every persisted file carries a [`StoreHeader`] (version,
//!   embedding-model fingerprint, embedding dimension). Boot MUST refuse
//!   to consume a file whose `embedder_id` differs from the currently
//!   wired embedder — swapping models without re-indexing produces
//!   meaningless cosine scores. The header is the single mechanism for
//!   that check; do not rely on file path or sibling metadata.
//! - `save()` is **synchronous** — only call it from a non-async context.
//!   Async paths MUST use [`VectorStore::snapshot_for_save`] +
//!   [`VectorStore::save_snapshot`] inside `tokio::task::spawn_blocking`
//!   (TRD §2.4 Golden Rule).
//! - The atomic-rename pair is the only durable write. A direct
//!   `fs::write` over `path` would race with concurrent `load()`.

use std::fs;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;

use crate::error::{MukeiError, Result};

/// Suffix appended to the live path while writing.
pub const ATOMIC_SUFFIX: &str = "swap";

/// On-disk format version. Bump whenever the [`Inner`] layout changes in
/// a way that breaks deserialisation. The boot path refuses any file
/// whose `format_version` is unknown.
pub const STORE_FORMAT_VERSION: u32 = 1;

/// Header persisted at the top of every store file. Carries the fields
/// needed to detect embedder / dimension drift across app upgrades.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct StoreHeader {
    /// Format version of the on-disk layout (see [`STORE_FORMAT_VERSION`]).
    pub format_version: u32,
    /// Stable identifier of the embedding model that produced these
    /// vectors (e.g. `"minilm-l6-v2:sha256:<hex>"`). Mismatch ⇒
    /// `MukeiError::ModelCorrupted` or a forced reindex.
    pub embedder_id: String,
    /// Embedding dimension. A mismatch with the live embedder is a
    /// fatal load error — cosine over differing dims is undefined.
    pub embedding_dim: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum VectorStoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("chunk {0} was not previously added")]
    ChunkIdNotInStore(u64),
}

impl From<VectorStoreError> for MukeiError {
    fn from(e: VectorStoreError) -> Self {
        match e {
            VectorStoreError::ChunkIdNotInStore(id) => MukeiError::Internal(format!("chunk {id} not in store")),
            other => MukeiError::Internal(other.to_string()),
        }
    }
}

/// In-memory mirror of the on-disk HNSW index. The `usearch` binding
/// itself is feature-gated; we provide a pure-Rust JSON-based store
/// so unit tests / sandbox builds work.
pub struct VectorStore {
    path: PathBuf,
    inner: Mutex<Inner>,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Inner {
    /// Persisted header. Optional only so V0 files (no header) can be
    /// detected during migration; new writes always carry a header.
    #[serde(default)]
    header: Option<StoreHeader>,
    vectors: Vec<(u64, Vec<f32>, String)>,
}

impl VectorStore {
    /// Open (or create) a vector store at `path`. Does not load —
    /// call [`Self::load`] explicitly.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), inner: Mutex::new(Inner::default()) }
    }

    pub fn path(&self) -> &Path { &self.path }

    /// Stamp the in-memory state with the embedder identity that produced
    /// these vectors. Must be called BEFORE the first `add()` of a fresh
    /// store, and re-checked on `load()`.
    pub fn set_header(&self, header: StoreHeader) {
        self.inner.lock().header = Some(header);
    }

    /// Read the persisted header, if any.
    pub fn header(&self) -> Option<StoreHeader> {
        self.inner.lock().header.clone()
    }

    /// Compatibility check used by the boot path. Returns `Err` if the
    /// store was previously written by a different embedder / dimension /
    /// on-disk format than the currently-wired one.
    pub fn assert_compatible_with(&self, expected: &StoreHeader) -> Result<()> {
        match self.header() {
            None => Ok(()), // empty / fresh store — caller must call set_header
            Some(found) if found == *expected => Ok(()),
            Some(found) => Err(MukeiError::ModelCorrupted).map_err(|e| {
                tracing::warn!(?found, ?expected, "vector store header mismatch — forcing reindex");
                e
            }),
        }
    }

    pub fn load(&self) -> Result<()> {
        match fs::read(&self.path) {
            Ok(bytes) => {
                let parsed: Inner = serde_json::from_slice(&bytes)
                    .map_err(|e| MukeiError::Internal(e.to_string()))?;
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

    pub fn add(&self, chunk_id: u64, vec: Vec<f32>, digest: String) {
        self.inner.lock().vectors.push((chunk_id, vec, digest));
    }

    pub fn remove(&self, chunk_id: u64) {
        self.inner.lock().vectors.retain(|(id, _, _)| *id != chunk_id);
    }

    pub fn count(&self) -> usize {
        self.inner.lock().vectors.len()
    }

    /// Save atomically. Writes to `<path>.<ATOMIC_SUFFIX>` first, then
    /// renames over the live path. Crash-between leaves the live file
    /// untouched.
    ///
    /// **Synchronous** — only call this from a non-async context, or
    /// (preferably) use [`Self::snapshot_for_save`] + [`Self::save_snapshot`]
    /// from an async task (TRD §2.4 Golden Rule: never block the runtime).
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

    /// Top-K cosine-similarity, returning chunk_id → score pairs.
    pub fn search(&self, q: &[f32], k: usize) -> Vec<(u64, f32)> {
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
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for i in 0..n {
        dot += a[i] * b[i];
        na  += a[i] * a[i];
        nb  += b[i] * b[i];
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

    #[test]
    fn save_creates_live_file() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("v.usearch");
        let s = VectorStore::open(&p);
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
        s.add(1, vec![1.0, 0.0], "d1".into());
        s.save().unwrap();
        let loaded = VectorStore::open(&p);
        loaded.load().unwrap();
        assert_eq!(loaded.count(), 1);
    }

    #[test]
    fn search_returns_top_k_cosine() {
        let s = VectorStore::open("/tmp/_unused.usearch");
        s.add(1, vec![1.0, 0.0], "d".into());
        s.add(2, vec![0.0, 1.0], "d".into());
        s.add(3, vec![0.7071, 0.7071], "d".into());
        let r = s.search(&[1.0, 0.0], 2);
        assert_eq!(r[0].0, 1); // exact match
    }

    #[test]
    fn remove_clears_id() {
        let s = VectorStore::open("/tmp/_unused.usearch");
        s.add(7, vec![1.0], "x".into());
        s.remove(7);
        assert_eq!(s.count(), 0);
    }
}
