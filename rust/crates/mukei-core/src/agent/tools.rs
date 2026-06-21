//! `mukei_core::agent::tools` — TRD §2.5.
//!
//! Hosts:
//!   * `MAX_FAILURES_PER_TOOL` constant (BUGFIX v0.7.4 — uniform across
//!     every tool, including `math_eval`).
//!   * `FailureTracker` — JSON-object-key-canonical SHA-256 fingerprint
//!     so `{"a":1,"b":2}` and `{"b":2,"a":1}` collide (PRD §6 / §2.5).
//!   * `ToolExecutor` — parallel tokio dispatcher with a `cancel_token`.

use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::error::{MukeiError, Result};

use crate::tools::validator::TypedToolCall;
use crate::types::ToolResult;

/// Maximum *consecutive* failures on the same fingerprint before the
/// tracker auto-blocks the tool for the rest of the turn. REQ-AGT-05.
pub const MAX_FAILURES_PER_TOOL: u32 = 2;

/// Per-tool abuse-prevention state.
#[derive(Default)]
pub struct FailureTracker {
    /// Maps `tool_name` → { fingerprint → consecutive_fail_count }.
    /// `parking_lot::Mutex` keeps the hot path single-digit µs.
    inner: Mutex<HashMap<String, HashMap<String, u32>>>,
}

impl FailureTracker {
    pub fn new() -> Self { Self::default() }

    /// Compute a SHA-256 fingerprint over the tool arguments with
    /// **JSON-object-key-canonical** ordering. Two payloads that
    /// differ only by key order collide.
    pub fn fingerprint(tool_name: &str, args: &serde_json::Value) -> String {
        let mut sorted = serde_json::Map::new();
        match args {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort();
                for k in keys {
                    sorted.insert(k.clone(), map[k].clone());
                }
            }
            _ => {}
        }
        let canonical = serde_json::Value::Object(sorted);

        let mut h = Sha256::new();
        h.update(tool_name.as_bytes());
        h.update([0u8]);
        let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
        h.update(&bytes);
        crate::diagnostics::crash_logger::hex_helper(&h.finalize())
    }

    /// Record a failure for `tool_name`+`fingerprint`. Returns `true`
    /// once the cumulative count has reached the abuse limit, in which
    /// case the agent core MUST abort the turn.
    pub fn record_failure(&self, tool_name: &str, fingerprint: &str) -> bool {
        let mut g = self.inner.lock();
        let per_tool: &mut HashMap<String, u32> = g.entry(tool_name.to_string()).or_default();
        let count = per_tool.entry(fingerprint.to_string()).or_insert(0);
        *count += 1;
        *count > MAX_FAILURES_PER_TOOL
    }

    pub fn reset(&self, tool_name: &str) {
        self.inner.lock().remove(tool_name);
    }
}

/// Concrete executor — owns the `FailureTracker` and dispatches into
/// `crate::tools` via its registry.
pub struct ToolExecutor {
    registry: Arc<crate::tools::ToolRegistry>,
    tracker:  Arc<FailureTracker>,
}

impl ToolExecutor {
    pub fn new(registry: Arc<crate::tools::ToolRegistry>, tracker: Arc<FailureTracker>) -> Self {
        Self { registry, tracker }
    }

    pub fn tracker(&self) -> &FailureTracker { &self.tracker }

    /// Run a set of validated tool calls **in parallel** via
    /// `tokio::join_all`. Cancellation propagates through
    /// `cancel_token`.
    pub async fn execute_parallel(
        &self,
        calls: Vec<TypedToolCall>,
        cancel_token: CancellationToken,
    ) -> Result<(Vec<ToolResult>, Option<MukeiError>)> {
        let mut handles = Vec::with_capacity(calls.len());

        for call in calls {
            let registry = self.registry.clone();
            let token = cancel_token.clone();
            handles.push(tokio::spawn(async move {
                let tool = registry.get(&call.name).ok_or_else(|| MukeiError::UnknownTool {
                    tool_name: call.name.clone(),
                })?;
                let started = std::time::Instant::now();
                let fp = FailureTracker::fingerprint(&call.name, &call.arguments);
                let result = tokio::select! {
                    res = tool.run(call.arguments.clone()) => res,
                    _ = token.cancelled() => Err(MukeiError::Cancelled),
                };
                Ok::<_, MukeiError>((call, fp, started.elapsed(), result))
            }));
        }

        let mut results = Vec::new();
        let mut blocked = None;
        for h in handles {
            match h.await {
                Ok(Ok((call, _fp, took, Ok(out)))) => results.push(ToolResult {
                    call_id: call.id,
                    name: call.name,
                    output: out,
                    ok: true,
                    took,
                    trust: "computed".to_string(),
                }),
                Ok(Ok((call, fp, _took, Err(e)))) => {
                    if self.tracker.record_failure(&call.name, &fp) {
                        blocked = Some(MukeiError::ToolAbuseBlocked { tool_name: call.name.clone() });
                    }
                    results.push(ToolResult {
                        call_id: call.id,
                        name: call.name,
                        output: format!("<error>{}</error>", e),
                        ok: false,
                        took: std::time::Duration::from_millis(0),
                        trust: "computed".to_string(),
                    });
                }
                Ok(Err(e)) => return Err(e),
                Err(e) => return Err(MukeiError::BlockingJoinFailed(e.to_string())),
            }
        }
        Ok((results, blocked))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_key_order_invariant() {
        let a = serde_json::json!({"a": 1, "b": 2});
        let b = serde_json::json!({"b": 2, "a": 1});
        assert_eq!(
            FailureTracker::fingerprint("x", &a),
            FailureTracker::fingerprint("x", &b),
        );
    }

    #[test]
    fn record_failure_blocks_after_two() {
        let t = FailureTracker::new();
        let fp: String = "x".into();
        assert!(!t.record_failure("tool", &fp)); // 1
        assert!(!t.record_failure("tool", &fp)); // 2
        assert!( t.record_failure("tool", &fp)); // 3 -> exceeds MAX
    }

    #[test]
    fn reset_clears_state() {
        let t = FailureTracker::new();
        t.record_failure("tool", "fp");
        t.reset("tool");
        assert!(!t.record_failure("tool", "fp"));
    }
}
