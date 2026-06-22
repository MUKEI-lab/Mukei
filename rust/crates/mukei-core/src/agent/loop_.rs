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
    pub(crate) tools:   ToolExecutor,
    pub(crate) watchdog: WatchdogHandle,
}

impl AgentLoop {
    pub fn new(
        context: ContextBudgetManager,
        tools: ToolExecutor,
        watchdog: WatchdogHandle,
    ) -> Arc<Self> {
        Arc::new(Self { context, tools, watchdog })
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

            // LLM turn — bridge crate injects the inference backend.
            let (assistant_text, used_tokens) =
                crate::engine::llama_wrapper::run_inference(
                    &context_text,
                    cancel_token.clone(),
                    token_sender.clone(),
                ).await?;

            tokens_so_far = tokens_so_far.saturating_add(used_tokens);

            if crate::engine::llama_wrapper::has_tool_call(&assistant_text) {
                // Tool dispatch path (TRD §2.3).
                let tool_calls =
                    crate::tools::validator::parse_gbnf_output(&assistant_text)?;
                let validated =
                    crate::tools::validator::validate_tool_calls(tool_calls)?;
                let (tool_outcomes, blocked) =
                    self.tools.execute_parallel(validated, cancel_token.clone()).await?;

                let assistant_id = MessageId::default();
                history.push(ChatMessage {
                    id: assistant_id,
                    role: Role::Assistant,
                    branch,
                    is_active: true,
                    created_at: chrono::Utc::now(),
                    content: assistant_text,
                    parent: Some(last_id),
                    token_count: Some(used_tokens as u32),
                });
                last_id = assistant_id;
                // Each ToolOutcome carries the typed result (success OR a
                // structured `<external_data source="tool_error">` envelope
                // with failure kind + remediation hint) plus an optional
                // no-progress backoff notice. We append BOTH so the LLM
                // sees the full picture on the next turn.
                for outcome in tool_outcomes {
                    let tool_id = MessageId::default();
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
                        let notice_id = MessageId::default();
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
                    let supervisor_id = MessageId::default();
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
                let final_id = MessageId::default();
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
