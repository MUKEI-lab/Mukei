//! `mukei_core::agent::loop_` — TRD §2.3.
//!
//! # Invariants (do NOT relax without a TRD amendment)
//!
//! - The ReAct loop is the **only** caller of [`crate::engine::llama_wrapper::run_inference`]
//!   inside `mukei-core`. Any other inference call site is a bug.
//! - **One watchdog source of truth.** Iteration / token / wall-time budgets are
//!   enforced **exclusively** by [`WatchdogHandle`]. The loop MUST NOT carry its
//!   own duplicate counter — having two separate limits creates config drift
//!   (PRD §24, REQ-AGT-04).
//! - On tool-abuse block: NEVER hard-abort the loop. Inject a structured
//!   `<external_data source="agent_supervisor">` directive and let the LLM
//!   produce a final answer from the context already gathered.
//! - Every assistant / tool turn appended to `history` MUST carry a real
//!   `parent` (last message in the current branch) and the **same** `BranchId`
//!   for the entire turn — a flat history breaks the branch graph and the
//!   recovery_state replay (BS §2 / V004__branching.sql).
//! - Tool-call detection delegates to [`crate::engine::llama_wrapper::has_tool_call`],
//!   which uses a token-aware parser, **not** a brace-counter.
//!
//! The bridge crate drives the loop through a
//! [`tokio_util::CancellationToken`] + the streaming mpsc `token_sender` that
//! QML listens to.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent::context::ContextBudgetManager;
use crate::agent::tools::ToolExecutor;
use crate::agent::tools::{FailureKind, ToolExecutionPolicy};
use crate::agent::watchdog::WatchdogHandle;
use crate::error::MukeiError;
use crate::types::{BranchId, ChatMessage, MessageId, Role};

/// Default reference value for the watchdog iteration budget. The
/// **authoritative** value at runtime lives in [`WatchdogHandle`] —
/// this constant exists only so the bridge crate and tests can share a
/// single literal when constructing the watchdog.
pub const DEFAULT_MAX_ITERATIONS: usize = 8;

/// Pure-Rust agent loop. Always driven through
/// [`tokio::task::spawn`] — it never blocks.
pub struct AgentLoop {
    pub(crate) context: ContextBudgetManager,
    pub(crate) tools: ToolExecutor,
    pub(crate) watchdog: WatchdogHandle,
}

impl AgentLoop {
    pub fn new(
        context: ContextBudgetManager,
        tools: ToolExecutor,
        watchdog: WatchdogHandle,
    ) -> Arc<Self> {
        Arc::new(Self {
            context,
            tools,
            watchdog,
        })
    }

    /// Run the loop until either the LLM returns a final answer, the
    /// watchdog trips, or `cancel_token` fires.
    ///
    /// `branch` is the active branch; every appended message uses the
    /// same branch id so the tree shape encoded in BS §2 / V004 is preserved.
    pub async fn run(
        self: Arc<Self>,
        user_input: String,
        branch: BranchId,
        cancel_token: CancellationToken,
        token_sender: mpsc::Sender<String>,
    ) -> Result<(), MukeiError> {
        // ---- Per-turn reset contract (Issues #4 / #5 / #6 / #7) ----
        // AgentLoop is constructed ONCE (model loading is expensive)
        // and reused across every user message. Several subsystems
        // were designed with per-turn reset in mind but were never
        // wired up. We wire them here, at THE turn boundary, so the
        // contract has exactly one enforcement point.
        self.watchdog.rearm(); // #6
        self.tools.reset_for_new_turn(); // #4 + #5
        crate::tools::hardware::HardwareTool::begin_turn(); // #7
        tracing::debug!("agent loop: per-turn subsystems rearmed");

        let mut history: Vec<ChatMessage> = vec![ChatMessage::user(branch, user_input)];
        // `last_id` tracks the parent pointer for the NEXT appended turn so
        // the conversation forms a real tree, not a flat list.
        let mut last_id: MessageId = history.last().expect("seeded above").id;
        let mut iteration: usize = 0;
        let mut tokens_so_far: u64 = 0;

        loop {
            // Single watchdog source of truth (TRD §2.6).
            self.watchdog.check(iteration, tokens_so_far)?;
            if cancel_token.is_cancelled() {
                return Ok(());
            }

            let context_text = self.context.build_for(&history).await?;

            // Architect review GH #46 + GH #47: wrap the inference call
            // in a `tokio::select!` over the cancel token AND the
            // watchdog's remaining wall-clock budget. A hung inference
            // can no longer outlive the agent-loop deadline even if
            // QML never delivers a cancel.
            let remaining = self.watchdog.remaining_wall_clock();
            let (assistant_text, used_tokens) = tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    tracing::debug!("agent loop: user cancelled mid-inference");
                    return Ok(());
                }
                _ = tokio::time::sleep(remaining), if !remaining.is_zero() => {
                    return Err(MukeiError::WatchdogExceeded { kind: "seconds" });
                }
                res = crate::engine::llama_wrapper::run_inference(
                    &context_text,
                    cancel_token.clone(),
                    token_sender.clone(),
                ) => res?,
            };

            tokens_so_far = tokens_so_far.saturating_add(used_tokens);

            if crate::engine::llama_wrapper::has_tool_call(&assistant_text) {
                // Tool dispatch path (TRD §2.3).
                //
                // Issue #10 (CRITICAL): NEVER hard-abort the turn on a
                // parse / validation error. The loop's own header says
                // "on tool-abuse block: NEVER hard-abort" — the same
                // contract must hold for malformed tool calls. We:
                //   1. Push the assistant turn first (so the LLM's
                //      attempt is preserved in history).
                //   2. Convert parse failures into a tool_error envelope
                //      and continue.
                //   3. Use the partial `validate()` helper so VALID calls
                //      in a mixed-validity batch still execute; invalid
                //      ones become structured envelopes.
                // Architect review GH #2 + GH #11: use the explicit
                // `MessageId::new()` constructor (clearer than
                // `Default::default()`) and assert the branch invariant
                // before pushing.
                let assistant_id = MessageId::new();
                debug_assert_eq!(
                    history.last().map(|m| m.branch),
                    Some(branch),
                    "branch invariant violated: parent message is on a different branch",
                );
                history.push(ChatMessage {
                    id: assistant_id,
                    role: Role::Assistant,
                    branch,
                    is_active: true,
                    created_at: chrono::Utc::now(),
                    content: assistant_text.clone(),
                    parent: Some(last_id),
                    token_count: Some(used_tokens as u32),
                });
                last_id = assistant_id;

                let raw_calls = match crate::tools::validator::parse_gbnf_output(&assistant_text) {
                    Ok(calls) => calls,
                    Err(parse_err) => {
                        // Inject a structured envelope and let the LLM
                        // produce a final answer (or retry on next turn).
                        let envelope = crate::agent::tools::feedback::render_tool_error_envelope(
                            "validator",
                            &parse_err,
                            FailureKind::Validation,
                            1,
                            ToolExecutionPolicy::DEFAULT_MAX_FAILURES,
                        );
                        let parse_id = MessageId::new();
                        debug_assert_eq!(
                            history.last().map(|m| m.branch),
                            Some(branch),
                            "branch invariant violated on parse-error envelope push",
                        );
                        history.push(ChatMessage {
                            id: parse_id,
                            role: Role::Tool,
                            branch,
                            is_active: true,
                            created_at: chrono::Utc::now(),
                            content: envelope,
                            parent: Some(last_id),
                            token_count: None,
                        });
                        last_id = parse_id;
                        tracing::warn!(err = ?parse_err, "tool-call parse failed — graceful degrade, LLM retries from envelope");
                        iteration += 1;
                        continue;
                    }
                };

                // Partial validation: execute the ACCEPTED calls and
                // turn each rejection into a per-call tool_error envelope.
                let (validated, validation_errors) = crate::tools::validator::validate(raw_calls);
                for verr in &validation_errors {
                    let rejection_err = MukeiError::ToolArgsRejected {
                        tool_name: "validator".to_string(),
                        reason: crate::tools::validator::format_for_llm(std::slice::from_ref(verr)),
                    };
                    let envelope = crate::agent::tools::feedback::render_tool_error_envelope(
                        "validator",
                        &rejection_err,
                        FailureKind::Validation,
                        1,
                        ToolExecutionPolicy::DEFAULT_MAX_FAILURES,
                    );
                    let rejection_id = MessageId::new();
                    debug_assert_eq!(
                        history.last().map(|m| m.branch),
                        Some(branch),
                        "branch invariant violated on validator-rejection envelope push",
                    );
                    history.push(ChatMessage {
                        id: rejection_id,
                        role: Role::Tool,
                        branch,
                        is_active: true,
                        created_at: chrono::Utc::now(),
                        content: envelope,
                        parent: Some(last_id),
                        token_count: None,
                    });
                    last_id = rejection_id;
                }
                tracing::debug!(
                    accepted = validated.len(),
                    rejected = validation_errors.len(),
                    "validator partition"
                );
                if validated.is_empty() {
                    // Nothing executable left to dispatch — give the
                    // model a chance to recover on the next turn.
                    iteration += 1;
                    continue;
                }
                let (tool_outcomes, blocked) = self
                    .tools
                    .execute_parallel(validated, cancel_token.clone())
                    .await?;

                // Each ToolOutcome carries the typed result (success OR a
                // structured `<external_data source="tool_error">` envelope
                // with failure kind + remediation hint) plus an optional
                // no-progress backoff notice. We append BOTH so the LLM
                // sees the full picture on the next turn.
                for outcome in tool_outcomes {
                    let tool_id = MessageId::new();
                    debug_assert_eq!(
                        history.last().map(|m| m.branch),
                        Some(branch),
                        "branch invariant violated on tool-outcome push",
                    );
                    history.push(ChatMessage {
                        id: tool_id,
                        role: Role::Tool,
                        branch,
                        is_active: true,
                        created_at: chrono::Utc::now(),
                        content: outcome.result.output,
                        parent: Some(last_id),
                        token_count: None,
                    });
                    last_id = tool_id;

                    if let Some(notice) = outcome.repeat_output_notice {
                        let notice_id = MessageId::new();
                        debug_assert_eq!(
                            history.last().map(|m| m.branch),
                            Some(branch),
                            "branch invariant violated on repeat-output notice push",
                        );
                        history.push(ChatMessage {
                            id: notice_id,
                            role: Role::Tool,
                            branch,
                            is_active: true,
                            created_at: chrono::Utc::now(),
                            content: notice,
                            parent: Some(last_id),
                            token_count: None,
                        });
                        last_id = notice_id;
                    }
                }
                // PRD REQ-AGT-04: when a tool is abuse-blocked we MUST NOT
                // hard-abort the loop. The error is injected as a structured
                // Tool turn so the LLM gets one more chance to respond using
                // the context already gathered, and the user sees a final
                // assistant answer instead of a UI error toast.
                if let Some(block_err) = blocked {
                    let blocked_tool = match &block_err {
                        MukeiError::ToolAbuseBlocked { tool_name }
                        | MukeiError::ToolPermanentlyDisabled { tool_name }
                        | MukeiError::UnknownTool { tool_name } => tool_name.clone(),
                        _ => "<unknown>".to_string(),
                    };
                    let directive = crate::agent::tools::render_supervisor_directive(
                        &blocked_tool,
                        block_err.error_code(),
                    );
                    let supervisor_id = MessageId::new();
                    debug_assert_eq!(
                        history.last().map(|m| m.branch),
                        Some(branch),
                        "branch invariant violated on supervisor-directive push",
                    );
                    history.push(ChatMessage {
                        id: supervisor_id,
                        role: Role::Tool,
                        branch,
                        is_active: true,
                        created_at: chrono::Utc::now(),
                        content: directive,
                        parent: Some(last_id),
                        token_count: None,
                    });
                    last_id = supervisor_id;
                    tracing::warn!(tool = %blocked_tool, code = block_err.error_code(), "tool blocked — graceful degrade, LLM will answer from gathered context");
                }
                iteration += 1;
                continue;
            } else {
                // Final answer — push the assistant turn (parent-linked to
                // the previous turn in the active branch) and bail.
                let final_id = MessageId::new();
                debug_assert_eq!(
                    history.last().map(|m| m.branch),
                    Some(branch),
                    "branch invariant violated on final-answer push",
                );
                history.push(ChatMessage {
                    id: final_id,
                    role: Role::Assistant,
                    branch,
                    is_active: true,
                    created_at: chrono::Utc::now(),
                    content: assistant_text,
                    parent: Some(last_id),
                    token_count: Some(used_tokens as u32),
                });
                let _ = final_id;
                return Ok(());
            }
        }
    }
}

/// Cheap cloneable handle — `Arc<AgentLoop>`-equivalent ergonomic
/// everywhere.
pub type AgentLoopHandle = Arc<AgentLoop>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_default_config() {
        assert_eq!(DEFAULT_MAX_ITERATIONS, 8);
    }
}
