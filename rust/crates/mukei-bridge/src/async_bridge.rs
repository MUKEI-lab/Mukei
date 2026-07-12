//! Non-chat asynchronous bridge request coordination.
//!
//! QML-facing methods that may touch SQLite, the filesystem, or another
//! potentially blocking service must return an accepted request immediately.
//! Completion is delivered later through the bridge `async_result` signal.
//!
//! The tracker provides monotonically increasing request identifiers and a
//! per-domain generation. Callers must only apply completions for the latest
//! generation of a domain. This prevents a delayed response from replacing a
//! newer last-known-good projection.
//!
//! SOL 03 I/O inventory:
//! - synchronous: pure in-memory getters and bounded serialization only;
//! - asynchronous here: recovery, UI-session/draft, download, settings,
//!   storage, and private-document projections/mutations;
//! - intentionally excluded by ownership: chat/conversation protocol,
//!   operation/model-store snapshots, and diagnostics-store export plumbing.
//!   Those surfaces retain their existing contracts until their owning package
//!   can change both native and QML sides atomically.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AsyncRequestTicket {
    pub(crate) request_id: String,
    pub(crate) domain: String,
    pub(crate) generation: u64,
}

#[derive(Default)]
pub(crate) struct AsyncRequestTracker {
    next_request: AtomicU64,
    latest_generation: Mutex<HashMap<String, u64>>,
}

impl AsyncRequestTracker {
    pub(crate) fn accept(&self, domain: impl Into<String>) -> AsyncRequestTicket {
        let domain = domain.into();
        let sequence = self.next_request.fetch_add(1, Ordering::AcqRel) + 1;
        let generation = {
            let mut latest = self.latest_generation.lock();
            let next = latest.get(&domain).copied().unwrap_or(0).saturating_add(1);
            latest.insert(domain.clone(), next);
            next
        };
        AsyncRequestTicket {
            request_id: format!("{domain}:{sequence}"),
            domain,
            generation,
        }
    }

    pub(crate) fn is_current(&self, ticket: &AsyncRequestTicket) -> bool {
        self.latest_generation.lock().get(&ticket.domain).copied() == Some(ticket.generation)
    }

    pub(crate) fn accepted_json(&self, ticket: &AsyncRequestTicket) -> String {
        serde_json::json!({
            "schema_version": 1,
            "ok": true,
            "accepted": true,
            "request_id": ticket.request_id,
            "domain": ticket.domain,
            "generation": ticket.generation,
        })
        .to_string()
    }

    pub(crate) fn completion_json<T: Serialize>(
        &self,
        ticket: &AsyncRequestTicket,
        result: Result<T, serde_json::Value>,
    ) -> String {
        let current = self.is_current(ticket);
        match result {
            Ok(payload) => serde_json::json!({
                "schema_version": 1,
                "request_id": ticket.request_id,
                "domain": ticket.domain,
                "generation": ticket.generation,
                "current": current,
                "ok": true,
                "payload": payload,
            })
            .to_string(),
            Err(error) => serde_json::json!({
                "schema_version": 1,
                "request_id": ticket.request_id,
                "domain": ticket.domain,
                "generation": ticket.generation,
                "current": current,
                "ok": false,
                "error": error,
            })
            .to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sol03_async_request_returns_accepted_without_waiting_for_completion() {
        let tracker = AsyncRequestTracker::default();
        let ticket = tracker.accept("settings.snapshot");
        let accepted: serde_json::Value =
            serde_json::from_str(&tracker.accepted_json(&ticket)).unwrap();
        assert_eq!(accepted["accepted"], true);
        assert_eq!(accepted["domain"], "settings.snapshot");
        assert!(accepted["request_id"]
            .as_str()
            .unwrap()
            .starts_with("settings.snapshot:"));
    }

    #[test]
    fn sol03_async_completion_keeps_request_correlation_after_delay() {
        let tracker = AsyncRequestTracker::default();
        let ticket = tracker.accept("documents.snapshot");
        let completed: serde_json::Value = serde_json::from_str(
            &tracker.completion_json(&ticket, Ok(serde_json::json!([{"id": "doc"}]))),
        )
        .unwrap();
        assert_eq!(completed["request_id"], ticket.request_id);
        assert_eq!(completed["generation"], ticket.generation);
        assert_eq!(completed["current"], true);
    }

    #[test]
    fn sol03_stale_completion_cannot_be_marked_current() {
        let tracker = AsyncRequestTracker::default();
        let first = tracker.accept("storage.snapshot");
        let second = tracker.accept("storage.snapshot");
        assert!(!tracker.is_current(&first));
        assert!(tracker.is_current(&second));
        let stale: serde_json::Value = serde_json::from_str(
            &tracker.completion_json(&first, Ok(serde_json::json!({"total": 1}))),
        )
        .unwrap();
        assert_eq!(stale["current"], false);
    }
}
