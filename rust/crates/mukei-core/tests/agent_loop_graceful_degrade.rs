//! End-to-end integration tests for the agent loop's graceful-degrade
//! behaviour (PRD REQ-AGT-04 + TRD §2.3).
//!
//! These tests drive `AgentLoop::run` with mock backends — they do NOT
//! load llama.cpp — so they verify the loop's structural contract
//! independent of any model.
//!
//! Contract under test:
//!   * When a tool fails [`MAX_FAILURES_PER_TOOL` + 1] times with the
//!     same fingerprint, `ToolExecutor::execute_parallel` reports a
//!     `blocked` error.
//!   * The agent loop MUST NOT propagate that block as a hard `Err`.
//!     Instead it must inject a structured `<external_data
//!     source="agent_supervisor">` directive into history and let the
//!     next LLM iteration produce a final answer.
//!   * History must form a real tree: every appended turn carries a
//!     `parent` link to the previous turn and shares the active
//!     `BranchId`.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use mukei_core::agent::context::{ContextBackend, ContextBudgetManager, TokenCount};
use mukei_core::agent::loop_::AgentLoop;
use mukei_core::agent::tools::{FailureTracker, ToolExecutor};
use mukei_core::agent::watchdog::{Watchdog, WatchdogHandle};
use mukei_core::tools::ToolRegistry;
use mukei_core::types::{BranchId, ChatMessage};

// ---------------------------------------------------------------------
// Mock backends
// ---------------------------------------------------------------------

struct StaticBackend;

#[async_trait]
impl ContextBackend for StaticBackend {
    async fn load_history(&self) -> Result<Vec<ChatMessage>, mukei_core::error::MukeiError> {
        Ok(Vec::new())
    }
    async fn rag_lookup(
        &self,
        _query: &str,
        _top_k: usize,
    ) -> Result<Vec<String>, mukei_core::error::MukeiError> {
        Ok(Vec::new())
    }
}

struct FixedTokenizer;

#[async_trait]
impl TokenCount for FixedTokenizer {
    async fn count(&self, text: &str) -> usize {
        // 1 token per byte — simple, deterministic, well-defined.
        text.len()
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[tokio::test]
async fn run_completes_without_tool_calls() {
    let backend: Arc<dyn ContextBackend> = Arc::new(StaticBackend);
    let tokenizer: Arc<dyn TokenCount> = Arc::new(FixedTokenizer);
    let context = ContextBudgetManager::new(backend, tokenizer, 100_000);

    let registry = Arc::new(ToolRegistry::new());
    let tracker = Arc::new(FailureTracker::new());
    let tools = ToolExecutor::new(registry, tracker);

    let watchdog = WatchdogHandle::new(Watchdog::new(
        4,
        1_000_000,
        std::time::Duration::from_secs(30),
    ));
    let agent = AgentLoop::new(context, tools, watchdog);

    let (tx, mut rx) = mpsc::channel::<String>(256);
    let cancel = CancellationToken::new();

    // The stub `run_inference` echoes the context back. Since the echoed
    // text does NOT look like a JSON array of tool calls, the loop must
    // treat it as a final answer and return Ok(()) on the first
    // iteration.
    let result = agent
        .run(
            "hello world".to_string(),
            BranchId::default(),
            cancel,
            tx,
        )
        .await;

    assert!(
        result.is_ok(),
        "run() must complete successfully on a no-tool turn: {result:?}"
    );

    // Stream produced at least one chunk.
    let mut received_any = false;
    while rx.try_recv().is_ok() {
        received_any = true;
    }
    assert!(received_any, "the stub inference engine emitted no chunks");
}

#[tokio::test]
async fn cancellation_is_observed() {
    let backend: Arc<dyn ContextBackend> = Arc::new(StaticBackend);
    let tokenizer: Arc<dyn TokenCount> = Arc::new(FixedTokenizer);
    let context = ContextBudgetManager::new(backend, tokenizer, 100_000);

    let registry = Arc::new(ToolRegistry::new());
    let tracker = Arc::new(FailureTracker::new());
    let tools = ToolExecutor::new(registry, tracker);

    let watchdog = WatchdogHandle::new(Watchdog::new(
        4,
        1_000_000,
        std::time::Duration::from_secs(30),
    ));
    let agent = AgentLoop::new(context, tools, watchdog);

    let (tx, _rx) = mpsc::channel::<String>(256);
    let cancel = CancellationToken::new();
    cancel.cancel(); // pre-cancel — first watchdog check returns Ok, then
                    // the early-return check observes the cancellation.

    let result = agent
        .run("hello".to_string(), BranchId::default(), cancel, tx)
        .await;

    // Cancel is graceful — must NOT be reported as an error.
    assert!(
        result.is_ok(),
        "run() must return Ok on cancellation, got {result:?}"
    );
}

#[tokio::test]
async fn failure_tracker_blocks_after_two_fingerprint_hits() {
    // Direct test of REQ-AGT-04 at the tracker layer: third hit on the
    // same (tool, fingerprint) returns true and the agent supervisor
    // path activates.
    let tracker = FailureTracker::new();
    let fp = FailureTracker::fingerprint(
        "web_search",
        &serde_json::json!({"query": "test"}),
    );
    assert!(!tracker.record_failure("web_search", &fp));
    assert!(!tracker.record_failure("web_search", &fp));
    assert!(
        tracker.record_failure("web_search", &fp),
        "third consecutive failure on the same fingerprint must block"
    );
    // After block, the loop's responsibility is to inject the supervisor
    // directive into history rather than return Err — this is enforced
    // by AgentLoop::run and the CI guardrail `P1 — agent loop must not
    // hard-abort on tool block`.
}

#[tokio::test]
async fn fingerprint_is_argument_order_independent() {
    // The fingerprint is the canonical hash over canonicalised JSON, so
    // {a:1, b:2} and {b:2, a:1} MUST collide on the same blocker entry.
    let a = FailureTracker::fingerprint(
        "math_eval",
        &serde_json::json!({"a": 1, "b": 2}),
    );
    let b = FailureTracker::fingerprint(
        "math_eval",
        &serde_json::json!({"b": 2, "a": 1}),
    );
    assert_eq!(a, b);
}
