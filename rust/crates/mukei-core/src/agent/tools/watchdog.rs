//! Same-output / no-progress detection — TRD §2.5, v0.7.5 audit P7.
//!
//! Tracks the last N successful outputs per `(tool, fingerprint)` so the
//! [`super::ToolExecutor`] can detect a no-progress loop: the model
//! keeps issuing the same tool call and the tool keeps returning the
//! same answer, but the conversation is making no progress.
//!
//! When the ring is full AND every entry hashes to the same value, the
//! detector fires. The executor then surfaces a [`FailureKind::Abuse`]
//! instant-block (see [`super::policy`]).

use std::collections::{HashMap, VecDeque};

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use crate::diagnostics::crash_logger::hex_helper;

/// Bounded ring per `(tool, fingerprint)` storing the SHA-256 of recent
/// outputs. Fires `true` when the ring is full AND every entry matches.
#[derive(Default)]
pub struct OutputRepeatTracker {
    /// `(tool, fingerprint)` → ring of recent output SHA-256 hashes.
    history: Mutex<HashMap<(String, String), VecDeque<String>>>,
}

impl OutputRepeatTracker {
    /// Construct an empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a fresh output and report whether the same byte sequence
    /// has now been observed `window` times in a row.
    ///
    /// `window == 0` disables the detector (always returns `false`).
    pub fn record_and_check(&self, tool: &str, fp: &str, output: &str, window: usize) -> bool {
        if window == 0 {
            return false;
        }
        let hash = {
            let mut h = Sha256::new();
            h.update(output.as_bytes());
            hex_helper(&h.finalize())
        };
        let mut g = self.history.lock();
        let ring = g.entry((tool.to_string(), fp.to_string())).or_default();
        ring.push_back(hash.clone());
        while ring.len() > window {
            ring.pop_front();
        }
        // Stuck iff the ring is full AND every entry matches.
        ring.len() == window && ring.iter().all(|h| h == &hash)
    }

    /// Forget the ring for a single `(tool, fingerprint)` pair (e.g.
    /// after a permanent / abuse block clears the rest of the turn).
    pub fn forget(&self, tool: &str, fp: &str) {
        self.history.lock().remove(&(tool.to_string(), fp.to_string()));
    }

    /// Drop every tracked pair. Called at the start of each new run().
    pub fn clear(&self) {
        self.history.lock().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_when_ring_full_and_all_equal() {
        let r = OutputRepeatTracker::new();
        assert!(!r.record_and_check("t", "fp", "same", 2));
        assert!(r.record_and_check("t", "fp", "same", 2));
    }

    #[test]
    fn does_not_fire_when_outputs_differ() {
        let r = OutputRepeatTracker::new();
        assert!(!r.record_and_check("t", "fp", "a", 2));
        assert!(!r.record_and_check("t", "fp", "b", 2));
        assert!(!r.record_and_check("t", "fp", "a", 2));
    }

    #[test]
    fn window_zero_is_noop() {
        let r = OutputRepeatTracker::new();
        for _ in 0..5 {
            assert!(!r.record_and_check("t", "fp", "x", 0));
        }
    }

    #[test]
    fn forget_clears_ring_for_pair() {
        let r = OutputRepeatTracker::new();
        r.record_and_check("t", "fp", "x", 2);
        r.record_and_check("t", "fp", "x", 2);
        r.forget("t", "fp");
        // After forget, the next push starts a fresh ring \u2014 it must NOT
        // immediately fire because the ring is no longer full.
        assert!(!r.record_and_check("t", "fp", "x", 2));
    }

    #[test]
    fn different_pairs_are_independent() {
        let r = OutputRepeatTracker::new();
        assert!(!r.record_and_check("t1", "fp", "x", 2));
        assert!(!r.record_and_check("t2", "fp", "x", 2));
        // Each ring fills independently.
        assert!(r.record_and_check("t1", "fp", "x", 2));
        assert!(r.record_and_check("t2", "fp", "x", 2));
    }
}
