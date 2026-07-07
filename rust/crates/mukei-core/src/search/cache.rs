//! Search cache — migration §12 (architect review GH #28).
//!
//! Simple in-memory `(SHA-256(query) + engine + task_kind) → results`
//! cache with per-task TTLs. **Process-local and ephemeral**; nothing
//! is persisted to disk.
//!
//! # Eviction & persistence policy
//!
//! For a privacy-critical, zero-telemetry product, the cache contract
//! must be explicit. The behaviour below is the canonical reference.
//!
//! 1. **In-memory only.** The cache lives in process heap, behind a
//!    `parking_lot::Mutex`. There is no file-backed sidecar from this
//!    crate. The bridge crate is free to layer a disk-backed cache
//!    underneath, but doing so MUST opt in to encryption (TRD §12.3
//!    wrapping-key pattern) and is out of scope for `mukei-core`.
//!
//! 2. **No persistence across process restart.** Process termination
//!    (cold kill, OOM, panic, ordinary exit) drops every entry. This
//!    is intentional: search results may quote user queries verbatim,
//!    so they MUST NOT outlive the runtime that produced them.
//!
//! 3. **TTL-based lazy eviction.** Every entry carries the TTL of its
//!    [`CacheKind`] (migration §12 defaults). An entry is treated as
//!    absent the moment `Entry::inserted.elapsed() > entry.ttl`;
//!    the actual heap slot is freed on the next [`SearchCache::get`]
//!    that observes the expiry, or on the next [`SearchCache::put`]
//!    that needs to enforce the capacity bound (see #4 below).
//!
//! 4. **Hard capacity bound — `MAX_ENTRIES = 512`.** When `put` is
//!    called on a full table, the cache runs an in-line sweep that
//!    (a) drops every expired entry, then (b) drops the
//!    least-recently-inserted entry until the table fits under the
//!    cap. This bounds worst-case memory at
//!    `MAX_ENTRIES * (key + result list)` so a runaway agent loop
//!    cannot leak unbounded RAM via repeated unique queries.
//!
//! 5. **Manual flush — [`SearchCache::clear`]** drops every entry
//!    immediately. Exposed for the diagnostics "Clear cache" button
//!    (BS §12).
//!
//! 6. **Periodic sweep is the bridge crate's responsibility.** This
//!    module never spawns a background sweeper task — the agent
//!    runtime decides cadence (typically tied to thermal / battery
//!    state via REQ-HW-04).
//!
//! # Invariants
//!
//! - The cache key is the SHA-256 of `task_kind || query || engine` so
//!   the same query for two different tasks (e.g. NEWS vs RESEARCH)
//!   does NOT collide.
//! - TTLs follow migration §12 defaults: 24 h for facts, 10 min for
//!   news, 1 h for research, 1 h for everything else.
//! - Capacity is bounded by [`MAX_ENTRIES`].

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use crate::search::engines::SearchEngineKind;
use crate::search::intent::TaskKind;
use crate::search::SearchHit;

/// Hard upper bound on the number of cache entries kept in memory.
///
/// See module-level docs for the eviction / persistence policy.
pub const MAX_ENTRIES: usize = 512;

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
    pub fn get(
        &self,
        task: TaskKind,
        engine: SearchEngineKind,
        query: &str,
    ) -> Option<Vec<SearchHit>> {
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
    ///
    /// If the table is at or above [`MAX_ENTRIES`] before the new entry
    /// is added, an in-line sweep runs:
    ///   1. every expired entry is dropped;
    ///   2. if still over the cap, the oldest-by-insertion entries are
    ///      dropped until `len < MAX_ENTRIES`.
    ///
    /// See module-level docs for the full eviction policy.
    pub fn put(&self, task: TaskKind, engine: SearchEngineKind, query: &str, hits: Vec<SearchHit>) {
        let key = Self::key(task, engine, query);
        let entry = Entry {
            hits,
            inserted: Instant::now(),
            ttl: CacheKind::from_task(task).ttl(),
        };
        let mut g = self.inner.lock();
        if g.len() >= MAX_ENTRIES && !g.contains_key(&key) {
            // Pass 1: drop expired.
            g.retain(|_, e| e.inserted.elapsed() <= e.ttl);
            // Pass 2: if still over the cap, evict oldest-by-insertion.
            while g.len() >= MAX_ENTRIES {
                let oldest_key = g
                    .iter()
                    .min_by_key(|(_, e)| e.inserted)
                    .map(|(k, _)| k.clone());
                match oldest_key {
                    Some(k) => {
                        g.remove(&k);
                    }
                    None => break,
                }
            }
        }
        g.insert(key, entry);
    }

    /// Number of entries currently held. Exposed for diagnostics and
    /// the capacity-bound regression test.
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    /// True when no entries are held.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
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

    #[test]
    fn put_enforces_max_entries_capacity_bound() {
        // Architect review GH #28: the documented policy is
        // MAX_ENTRIES = 512. Exceeding it must trigger oldest-first
        // eviction so a runaway agent loop cannot leak unbounded RAM.
        let c = SearchCache::new();
        // Insert MAX_ENTRIES + 32 distinct queries; len() must never
        // exceed MAX_ENTRIES once the cap is enforced.
        for i in 0..(MAX_ENTRIES + 32) {
            let q = format!("query-{i}");
            c.put(TaskKind::Fact, SearchEngineKind::Brave, &q, vec![hit("a")]);
        }
        assert!(
            c.len() <= MAX_ENTRIES,
            "cache exceeded MAX_ENTRIES: len = {}",
            c.len()
        );
    }

    #[test]
    fn clear_empties_the_cache() {
        let c = SearchCache::new();
        c.put(TaskKind::Fact, SearchEngineKind::Brave, "q", vec![hit("a")]);
        assert!(!c.is_empty());
        c.clear();
        assert!(c.is_empty());
    }
}
