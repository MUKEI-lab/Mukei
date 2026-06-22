//! Search cache — migration §12.
//!
//! Simple in-memory `(SHA-256(query) + engine + task_kind) → results`
//! cache with per-task TTLs. The bridge crate may layer a disk-backed
//! cache underneath; this module keeps the algorithm honest.
//!
//! # Invariants
//!
//! - The cache key is the SHA-256 of `task_kind || query || engine` so
//!   the same query for two different tasks (e.g. NEWS vs RESEARCH)
//!   does NOT collide.
//! - TTLs follow migration §12 defaults: 24 h for facts, 10 min for
//!   news, 1 h for research.
//! - Eviction is lazy: expired entries are dropped on next access; a
//!   periodic sweep is the bridge crate's responsibility.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use crate::search::engines::SearchEngineKind;
use crate::search::intent::TaskKind;
use crate::search::SearchHit;

/// Category of cached entry — drives the TTL choice.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CacheKind {
    /// Simple facts. 24 hours.
    Fact,
    /// News. 10 minutes.
    News,
    /// Research / explainers. 1 hour.
    Research,
    /// Catch-all bucket. 1 hour.
    Other,
}

impl CacheKind {
    /// TTL for this cache bucket (migration §12).
    pub fn ttl(self) -> Duration {
        match self {
            Self::Fact => Duration::from_secs(24 * 60 * 60),
            Self::News => Duration::from_secs(10 * 60),
            Self::Research | Self::Other => Duration::from_secs(60 * 60),
        }
    }

    /// Project a [`TaskKind`] into its cache bucket.
    pub fn from_task(kind: TaskKind) -> Self {
        match kind {
            TaskKind::Fact => Self::Fact,
            TaskKind::News => Self::News,
            TaskKind::Research | TaskKind::Compare | TaskKind::Academic | TaskKind::MultiStep => {
                Self::Research
            }
            _ => Self::Other,
        }
    }
}

#[derive(Clone)]
struct Entry {
    hits: Vec<SearchHit>,
    inserted: Instant,
    ttl: Duration,
}

/// Single-process cache. Send-able across the runtime.
#[derive(Default)]
pub struct SearchCache {
    inner: Mutex<HashMap<String, Entry>>,
}

impl SearchCache {
    /// Construct an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stable key for the entry.
    pub fn key(task: TaskKind, engine: SearchEngineKind, query: &str) -> String {
        let mut h = Sha256::new();
        h.update(task.as_tag().as_bytes());
        h.update([0u8]);
        h.update(engine.as_tag().as_bytes());
        h.update([0u8]);
        h.update(query.as_bytes());
        crate::diagnostics::crash_logger::hex_helper(&h.finalize())
    }

    /// Look up a fresh entry. Returns `None` when expired or absent;
    /// expired entries are evicted as a side effect.
    pub fn get(&self, task: TaskKind, engine: SearchEngineKind, query: &str) -> Option<Vec<SearchHit>> {
        let key = Self::key(task, engine, query);
        let mut g = self.inner.lock();
        if let Some(entry) = g.get(&key) {
            if entry.inserted.elapsed() <= entry.ttl {
                return Some(entry.hits.clone());
            }
            g.remove(&key);
        }
        None
    }

    /// Insert a fresh batch of hits under the task / engine pair.
    pub fn put(&self, task: TaskKind, engine: SearchEngineKind, query: &str, hits: Vec<SearchHit>) {
        let key = Self::key(task, engine, query);
        let entry = Entry {
            hits,
            inserted: Instant::now(),
            ttl: CacheKind::from_task(task).ttl(),
        };
        self.inner.lock().insert(key, entry);
    }

    /// Drop everything.
    pub fn clear(&self) {
        self.inner.lock().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(t: &str) -> SearchHit {
        SearchHit::new(t, "https://example.com/x", "...", SearchEngineKind::Brave)
    }

    #[test]
    fn key_is_stable_for_same_inputs() {
        let a = SearchCache::key(TaskKind::Fact, SearchEngineKind::Brave, "hello");
        let b = SearchCache::key(TaskKind::Fact, SearchEngineKind::Brave, "hello");
        assert_eq!(a, b);
    }

    #[test]
    fn key_differs_per_task_and_engine() {
        let k1 = SearchCache::key(TaskKind::Fact, SearchEngineKind::Brave, "x");
        let k2 = SearchCache::key(TaskKind::News, SearchEngineKind::Brave, "x");
        let k3 = SearchCache::key(TaskKind::Fact, SearchEngineKind::Tavily, "x");
        assert_ne!(k1, k2);
        assert_ne!(k1, k3);
        assert_ne!(k2, k3);
    }

    #[test]
    fn put_and_get_round_trip() {
        let c = SearchCache::new();
        c.put(TaskKind::Fact, SearchEngineKind::Brave, "q", vec![hit("a")]);
        let got = c.get(TaskKind::Fact, SearchEngineKind::Brave, "q").unwrap();
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn ttl_defaults_match_migration() {
        assert_eq!(CacheKind::Fact.ttl().as_secs(), 24 * 60 * 60);
        assert_eq!(CacheKind::News.ttl().as_secs(), 10 * 60);
        assert_eq!(CacheKind::Research.ttl().as_secs(), 60 * 60);
    }
}
