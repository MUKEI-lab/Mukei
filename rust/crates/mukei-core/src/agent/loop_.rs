//! `mukei_core::agent::loop_` — TRD §2.3.
//!
//! # Invariants (do NOT relax without a TRD amendment)
//!
//! - The ReAct loop owns exactly one [`crate::engine::InferenceBackend`] for
//!   its lifetime and routes every generation iteration through that backend.
//!   Backend selection happens at construction time, never inside `run`.
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

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

use futures::FutureExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent::context::ContextBudgetManager;
use crate::agent::tools::ToolExecutor;
use crate::agent::tools::{FailureKind, ToolExecutionPolicy};
use crate::agent::watchdog::WatchdogHandle;
use crate::diagnostics::observability::{
    AttributeValue, EventScope, EventSeverity, FieldSensitivity, ObservabilityRecorder,
    OperationalEvent, OutcomeClass,
};
use crate::engine::{
    BackendKind, BackendUnavailableReason, InferenceBackend, InferenceOutcome,
    MockInferenceBackend, StopReason, UnavailableInferenceBackend,
};
use crate::error::MukeiError;
use crate::types::{BranchId, ChatMessage, ConversationId, MessageId, Role};

#[async_trait::async_trait]
pub trait AgentEventSink: Send + Sync {
    /// Persist an intermediate assistant/tool message before the loop
    /// advances to another inference iteration. Implementations must be
    /// durable: returning `Ok(())` means the message is committed.
    async fn persist_intermediate(&self, message: &ChatMessage) -> Result<(), MukeiError>;
}

#[derive(Clone)]
pub struct AgentRunRequest {
    pub user_input: String,
    pub conversation: ConversationId,
    pub branch: BranchId,
    pub user_message_id: MessageId,
    pub cancel_token: CancellationToken,
    pub token_sender: mpsc::Sender<String>,
    pub event_sink: Option<Arc<dyn AgentEventSink>>,
}

impl AgentRunRequest {
    /// Build a scoped agent request without a persistence sink.
    ///
    /// Boundary layers can attach durable persistence with
    /// [`Self::with_event_sink`] while tests and pure in-memory callers keep
    /// the common path compact and explicit.
    pub fn new(
        user_input: impl Into<String>,
        conversation: ConversationId,
        branch: BranchId,
        user_message_id: MessageId,
        cancel_token: CancellationToken,
        token_sender: mpsc::Sender<String>,
    ) -> Self {
        Self {
            user_input: user_input.into(),
            conversation,
            branch,
            user_message_id,
            cancel_token,
            token_sender,
            event_sink: None,
        }
    }

    /// Attach the optional durable event sink used by the bridge/runtime.
    pub fn with_event_sink(mut self, event_sink: Option<Arc<dyn AgentEventSink>>) -> Self {
        self.event_sink = event_sink;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRunOutcome {
    /// External id of the message that the durable final assistant row
    /// should point to. For a simple turn this is the user message; after
    /// tool use it is the final tool/supervisor message.
    pub final_parent: MessageId,
    /// Token count reported by the inference backend for the final answer.
    pub final_token_count: Option<u32>,
    /// Exact final assistant response returned by the inference backend.
    /// This deliberately excludes streamed tool-call attempts from earlier
    /// iterations, so the durable final row cannot accidentally concatenate
    /// hidden ReAct protocol output.
    pub final_content: Option<String>,
    /// True when the cancellation token ended the run gracefully.
    pub cancelled: bool,
}

/// Stable inference terminal taxonomy used by supervision and observability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InferenceFailureCategory {
    Cancellation,
    BackendUnavailable,
    ModelActivationFailure,
    ExecutionFailure,
    InternalPanic,
    Timeout,
}

impl InferenceFailureCategory {
    pub const fn as_tag(self) -> &'static str {
        match self {
            Self::Cancellation => "cancellation",
            Self::BackendUnavailable => "backend_unavailable",
            Self::ModelActivationFailure => "model_activation_failure",
            Self::ExecutionFailure => "inference_execution_failure",
            Self::InternalPanic => "internal_panic",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InferenceTerminalOutcome {
    Succeeded = 1,
    Cancelled = 2,
    Failed = 3,
}

/// Compare-and-set terminal latch. It makes duplicate or racing terminal
/// completions observable and rejectable without relying on happy-path order.
struct InferenceTerminalGuard {
    state: AtomicU8,
}

impl InferenceTerminalGuard {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(0),
        }
    }

    fn try_finish(&self, outcome: InferenceTerminalOutcome) -> bool {
        self.state
            .compare_exchange(0, outcome as u8, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

enum SupervisedInferenceResult {
    Outcome(InferenceOutcome),
    Cancelled,
}

struct ActiveRunGuard<'a> {
    active_generation: &'a AtomicU64,
    generation: u64,
}

impl Drop for ActiveRunGuard<'_> {
    fn drop(&mut self) {
        let _ = self.active_generation.compare_exchange(
            self.generation,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

async fn persist_intermediate(
    sink: &Option<Arc<dyn AgentEventSink>>,
    message: &ChatMessage,
) -> Result<(), MukeiError> {
    if let Some(sink) = sink {
        sink.persist_intermediate(message).await?;
    }
    Ok(())
}

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
    pub(crate) inference_backend: Arc<dyn InferenceBackend>,
    next_run_generation: AtomicU64,
    active_run_generation: AtomicU64,
    observability: Option<ObservabilityRecorder>,
}

impl AgentLoop {
    /// Legacy compatibility constructor. It deliberately installs an
    /// unavailable backend, never a mock. Production assembly should use
    /// [`Self::new_with_backend`] so backend selection is explicit.
    pub fn new(
        context: ContextBudgetManager,
        tools: ToolExecutor,
        watchdog: WatchdogHandle,
    ) -> Arc<Self> {
        Self::new_with_backend(
            context,
            tools,
            watchdog,
            Arc::new(UnavailableInferenceBackend::new_with_reason(
                "agent_backend_not_injected",
                BackendUnavailableReason::NotInjected,
            )),
        )
    }

    /// Primary constructor: the production backend dependency is explicit.
    pub fn new_with_backend(
        context: ContextBudgetManager,
        tools: ToolExecutor,
        watchdog: WatchdogHandle,
        inference_backend: Arc<dyn InferenceBackend>,
    ) -> Arc<Self> {
        Self::new_with_backend_and_observability(context, tools, watchdog, inference_backend, None)
    }

    /// Explicit test/development constructor. Mock identity remains visible and
    /// can never satisfy product-ready capability checks.
    pub fn new_with_mock_for_tests(
        context: ContextBudgetManager,
        tools: ToolExecutor,
        watchdog: WatchdogHandle,
    ) -> Arc<Self> {
        Self::new_with_backend(
            context,
            tools,
            watchdog,
            Arc::new(MockInferenceBackend::default()),
        )
    }

    pub fn new_with_backend_and_observability(
        context: ContextBudgetManager,
        tools: ToolExecutor,
        watchdog: WatchdogHandle,
        inference_backend: Arc<dyn InferenceBackend>,
        observability: Option<ObservabilityRecorder>,
    ) -> Arc<Self> {
        Arc::new(Self {
            context,
            tools,
            watchdog,
            inference_backend,
            next_run_generation: AtomicU64::new(0),
            active_run_generation: AtomicU64::new(0),
            observability,
        })
    }

    /// Compatibility alias for older explicit-injection call sites.
    pub fn with_inference_backend(
        context: ContextBudgetManager,
        tools: ToolExecutor,
        watchdog: WatchdogHandle,
        inference_backend: Arc<dyn InferenceBackend>,
    ) -> Arc<Self> {
        Self::new_with_backend(context, tools, watchdog, inference_backend)
    }

    pub fn backend_kind(&self) -> BackendKind {
        self.inference_backend.identity().kind
    }

    pub fn active_run_generation(&self) -> Option<u64> {
        match self.active_run_generation.load(Ordering::Acquire) {
            0 => None,
            generation => Some(generation),
        }
    }

    fn record_inference_terminal(
        &self,
        name: &str,
        outcome: OutcomeClass,
        category: Option<InferenceFailureCategory>,
    ) {
        let backend_kind = self.inference_backend.identity().kind;
        match (outcome, category) {
            (OutcomeClass::Success, _) => tracing::info!(
                backend_kind = backend_kind.as_tag(),
                event = name,
                "inference lifecycle event"
            ),
            (_, Some(category)) => tracing::warn!(
                backend_kind = backend_kind.as_tag(),
                failure_category = category.as_tag(),
                event = name,
                "inference lifecycle event"
            ),
            _ => tracing::debug!(
                backend_kind = backend_kind.as_tag(),
                event = name,
                "inference lifecycle event"
            ),
        }

        let Some(recorder) = self.observability.as_ref() else {
            return;
        };
        let severity = if outcome.is_success_like() {
            EventSeverity::Info
        } else {
            EventSeverity::Warn
        };
        let Ok(mut event) = OperationalEvent::new(
            name,
            "engine.inference",
            severity,
            outcome,
            EventScope::Essential,
        ) else {
            return;
        };
        let _ = event.push_attribute(
            "backend_kind",
            AttributeValue::Stable(backend_kind.as_tag().to_string()),
            FieldSensitivity::OperationalSafe,
        );
        if let Some(category) = category {
            let _ = event.push_attribute(
                "failure_category",
                AttributeValue::Stable(category.as_tag().to_string()),
                FieldSensitivity::OperationalSafe,
            );
        }
        let _ = recorder.record_event(event);
    }

    async fn run_inference_supervised(
        &self,
        context_text: &str,
        cancel_token: CancellationToken,
        token_sender: mpsc::Sender<String>,
        remaining: std::time::Duration,
    ) -> Result<SupervisedInferenceResult, MukeiError> {
        let terminal = InferenceTerminalGuard::new();
        self.record_inference_terminal("inference.start", OutcomeClass::Success, None);

        if remaining.is_zero() {
            let _ = terminal.try_finish(InferenceTerminalOutcome::Failed);
            self.record_inference_terminal(
                "inference.terminal",
                OutcomeClass::Timeout,
                Some(InferenceFailureCategory::Timeout),
            );
            return Err(MukeiError::WatchdogExceeded { kind: "seconds" });
        }

        let backend_future = AssertUnwindSafe(self.inference_backend.run(
            context_text,
            cancel_token.clone(),
            token_sender,
        ))
        .catch_unwind();
        tokio::pin!(backend_future);

        tokio::select! {
            biased;
            _ = cancel_token.cancelled() => {
                if terminal.try_finish(InferenceTerminalOutcome::Cancelled) {
                    self.record_inference_terminal(
                        "inference.terminal",
                        OutcomeClass::Cancelled,
                        Some(InferenceFailureCategory::Cancellation),
                    );
                }
                Ok(SupervisedInferenceResult::Cancelled)
            }
            _ = tokio::time::sleep(remaining) => {
                if terminal.try_finish(InferenceTerminalOutcome::Failed) {
                    self.record_inference_terminal(
                        "inference.terminal",
                        OutcomeClass::Timeout,
                        Some(InferenceFailureCategory::Timeout),
                    );
                }
                Err(MukeiError::WatchdogExceeded { kind: "seconds" })
            }
            result = &mut backend_future => {
                match result {
                    Err(_) => {
                        if terminal.try_finish(InferenceTerminalOutcome::Failed) {
                            self.record_inference_terminal(
                                "inference.terminal",
                                OutcomeClass::InternalFailure,
                                Some(InferenceFailureCategory::InternalPanic),
                            );
                        }
                        Err(MukeiError::Internal(
                            "inference execution failed because the backend panicked".to_string(),
                        ))
                    }
                    Ok(Err(error)) => {
                        let category = match &error {
                            MukeiError::ModelLoadFailed(_) | MukeiError::ModelCorrupted => {
                                let identity = self.inference_backend.identity();
                                match (identity.kind, identity.unavailable_reason) {
                                    (
                                        BackendKind::Unavailable,
                                        Some(BackendUnavailableReason::ActivationFailed),
                                    ) => InferenceFailureCategory::ModelActivationFailure,
                                    (BackendKind::Unavailable, _) => {
                                        InferenceFailureCategory::BackendUnavailable
                                    }
                                    _ => InferenceFailureCategory::ExecutionFailure,
                                }
                            }
                            MukeiError::WatchdogExceeded { .. } => InferenceFailureCategory::Timeout,
                            _ => InferenceFailureCategory::ExecutionFailure,
                        };
                        if terminal.try_finish(InferenceTerminalOutcome::Failed) {
                            self.record_inference_terminal(
                                "inference.terminal",
                                if category == InferenceFailureCategory::Timeout {
                                    OutcomeClass::Timeout
                                } else if matches!(
                                    category,
                                    InferenceFailureCategory::BackendUnavailable
                                        | InferenceFailureCategory::ModelActivationFailure
                                ) {
                                    OutcomeClass::Unavailable
                                } else {
                                    OutcomeClass::InternalFailure
                                },
                                Some(category),
                            );
                        }
                        Err(error)
                    }
                    Ok(Ok(outcome)) => {
                        match outcome.stop_reason {
                            StopReason::Completed => {
                                if terminal.try_finish(InferenceTerminalOutcome::Succeeded) {
                                    self.record_inference_terminal(
                                        "inference.terminal",
                                        OutcomeClass::Success,
                                        None,
                                    );
                                }
                                Ok(SupervisedInferenceResult::Outcome(outcome))
                            }
                            StopReason::UserStopped => {
                                if terminal.try_finish(InferenceTerminalOutcome::Cancelled) {
                                    self.record_inference_terminal(
                                        "inference.terminal",
                                        OutcomeClass::Cancelled,
                                        Some(InferenceFailureCategory::Cancellation),
                                    );
                                }
                                Ok(SupervisedInferenceResult::Cancelled)
                            }
                            StopReason::ThermalKill => {
                                if terminal.try_finish(InferenceTerminalOutcome::Failed) {
                                    self.record_inference_terminal(
                                        "inference.terminal",
                                        OutcomeClass::Unavailable,
                                        Some(InferenceFailureCategory::ExecutionFailure),
                                    );
                                }
                                Err(MukeiError::ThermalThrottle)
                            }
                            StopReason::OutOfMemory => {
                                if terminal.try_finish(InferenceTerminalOutcome::Failed) {
                                    self.record_inference_terminal(
                                        "inference.terminal",
                                        OutcomeClass::Unavailable,
                                        Some(InferenceFailureCategory::ExecutionFailure),
                                    );
                                }
                                Err(MukeiError::OOM)
                            }
                            StopReason::WatchdogTripped => {
                                if terminal.try_finish(InferenceTerminalOutcome::Failed) {
                                    self.record_inference_terminal(
                                        "inference.terminal",
                                        OutcomeClass::Timeout,
                                        Some(InferenceFailureCategory::Timeout),
                                    );
                                }
                                Err(MukeiError::WatchdogExceeded { kind: "seconds" })
                            }
                        }
                    }
                }
            }
        }
    }

    /// Run the loop until either the LLM returns a final answer, the
    /// watchdog trips, or `cancel_token` fires.
    ///
    /// `branch` is the active branch; every appended message uses the
    /// same branch id so the tree shape encoded in BS §2 / V004 is preserved.
    pub async fn run(
        self: Arc<Self>,
        request: AgentRunRequest,
    ) -> Result<AgentRunOutcome, MukeiError> {
        self.run_seeded(
            vec![ChatMessage::user_with_id(
                request.user_message_id,
                request.branch,
                request.user_input,
            )],
            request.conversation,
            request.branch,
            request.cancel_token,
            request.token_sender,
            request.event_sink,
        )
        .await
    }

    /// Transitional adapter for boundary code that still owns the request as
    /// separate values. New callers should prefer [`AgentRunRequest`] so
    /// conversation/branch scope cannot be reordered accidentally.
    // This compatibility adapter intentionally mirrors the legacy boundary.
    // New code should construct `AgentRunRequest`; keeping the adapter avoids a
    // downstream API break while the typed request migration completes.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_with_parts(
        self: Arc<Self>,
        user_input: String,
        conversation: ConversationId,
        branch: BranchId,
        user_message_id: MessageId,
        cancel_token: CancellationToken,
        token_sender: mpsc::Sender<String>,
        event_sink: Option<Arc<dyn AgentEventSink>>,
    ) -> Result<AgentRunOutcome, MukeiError> {
        self.run(
            AgentRunRequest::new(
                user_input,
                conversation,
                branch,
                user_message_id,
                cancel_token,
                token_sender,
            )
            .with_event_sink(event_sink),
        )
        .await
    }

    /// Run from a durable seed history. Recovery uses this entry point to
    /// include the interrupted assistant prefix without inserting another
    /// synthetic user message into the conversation.
    pub async fn run_seeded(
        self: Arc<Self>,
        history: Vec<ChatMessage>,
        conversation: ConversationId,
        branch: BranchId,
        cancel_token: CancellationToken,
        token_sender: mpsc::Sender<String>,
        event_sink: Option<Arc<dyn AgentEventSink>>,
    ) -> Result<AgentRunOutcome, MukeiError> {
        let generation = self
            .next_run_generation
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        self.active_run_generation
            .fetch_max(generation, Ordering::AcqRel);
        let _active_run_guard = ActiveRunGuard {
            active_generation: &self.active_run_generation,
            generation,
        };

        let future = self.clone().run_seeded_inner(
            history,
            conversation,
            branch,
            cancel_token,
            token_sender,
            event_sink,
        );
        let result = match AssertUnwindSafe(future).catch_unwind().await {
            Ok(result) => result,
            Err(_) => {
                self.record_inference_terminal(
                    "agent.run.panic",
                    OutcomeClass::InternalFailure,
                    Some(InferenceFailureCategory::InternalPanic),
                );
                tracing::error!(
                    generation,
                    panic_category = "agent_execution",
                    "agent execution panic converted to terminal failure"
                );
                Err(MukeiError::Internal(
                    "agent execution failed because an internal operation panicked".to_string(),
                ))
            }
        };

        if self.next_run_generation.load(Ordering::Acquire) != generation
            || self.active_run_generation.load(Ordering::Acquire) != generation
        {
            tracing::warn!(generation, "stale agent completion ignored");
            return Err(MukeiError::Internal(
                "stale agent completion ignored".to_string(),
            ));
        }
        result
    }

    async fn run_seeded_inner(
        self: Arc<Self>,
        mut history: Vec<ChatMessage>,
        conversation: ConversationId,
        branch: BranchId,
        cancel_token: CancellationToken,
        token_sender: mpsc::Sender<String>,
        event_sink: Option<Arc<dyn AgentEventSink>>,
    ) -> Result<AgentRunOutcome, MukeiError> {
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

        if history.is_empty() {
            return Err(MukeiError::Invariant(
                "agent run requires at least one durable seed message".to_string(),
            ));
        }
        if history.iter().any(|message| message.branch != branch) {
            return Err(MukeiError::Invariant(
                "agent recovery seed spans multiple branches".to_string(),
            ));
        }
        // `last_id` tracks the parent pointer for the NEXT appended turn so
        // the conversation forms a real tree, not a flat list.
        let mut last_id: MessageId = history.last().expect("non-empty seed checked above").id;
        let mut iteration: usize = 0;
        let mut tokens_so_far: u64 = 0;

        loop {
            // Single watchdog source of truth (TRD §2.6).
            self.watchdog.check(iteration, tokens_so_far)?;
            if cancel_token.is_cancelled() {
                return Ok(AgentRunOutcome {
                    final_parent: last_id,
                    final_token_count: None,
                    final_content: None,
                    cancelled: true,
                });
            }

            let context_text = self
                .context
                .build_for(conversation, branch, &history)
                .await?;

            // Supervise every inference attempt so cancellation, timeout,
            // backend failure, and panic converge on exactly one terminal
            // outcome. The helper never exposes panic payloads.
            let remaining = self.watchdog.remaining_wall_clock();
            let inference_outcome = match self
                .run_inference_supervised(
                    &context_text,
                    cancel_token.clone(),
                    token_sender.clone(),
                    remaining,
                )
                .await?
            {
                SupervisedInferenceResult::Outcome(outcome) => outcome,
                SupervisedInferenceResult::Cancelled => {
                    return Ok(AgentRunOutcome {
                        final_parent: last_id,
                        final_token_count: None,
                        final_content: None,
                        cancelled: true,
                    });
                }
            };
            let assistant_text = inference_outcome.assistant_text;
            let used_tokens = inference_outcome.used_tokens;

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
                let assistant_message = ChatMessage {
                    id: assistant_id,
                    role: Role::Assistant,
                    branch,
                    is_active: true,
                    created_at: chrono::Utc::now(),
                    content: assistant_text.clone(),
                    parent: Some(last_id),
                    token_count: Some(used_tokens as u32),
                };
                persist_intermediate(&event_sink, &assistant_message).await?;
                history.push(assistant_message);
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
                        let parse_message = ChatMessage {
                            id: parse_id,
                            role: Role::Tool,
                            branch,
                            is_active: true,
                            created_at: chrono::Utc::now(),
                            content: envelope,
                            parent: Some(last_id),
                            token_count: None,
                        };
                        persist_intermediate(&event_sink, &parse_message).await?;
                        history.push(parse_message);
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
                    let rejection_message = ChatMessage {
                        id: rejection_id,
                        role: Role::Tool,
                        branch,
                        is_active: true,
                        created_at: chrono::Utc::now(),
                        content: envelope,
                        parent: Some(last_id),
                        token_count: None,
                    };
                    persist_intermediate(&event_sink, &rejection_message).await?;
                    history.push(rejection_message);
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
                    let tool_message = ChatMessage {
                        id: tool_id,
                        role: Role::Tool,
                        branch,
                        is_active: true,
                        created_at: chrono::Utc::now(),
                        content: outcome.result.output,
                        parent: Some(last_id),
                        token_count: None,
                    };
                    persist_intermediate(&event_sink, &tool_message).await?;
                    history.push(tool_message);
                    last_id = tool_id;

                    if let Some(notice) = outcome.repeat_output_notice {
                        let notice_id = MessageId::new();
                        debug_assert_eq!(
                            history.last().map(|m| m.branch),
                            Some(branch),
                            "branch invariant violated on repeat-output notice push",
                        );
                        let notice_message = ChatMessage {
                            id: notice_id,
                            role: Role::Tool,
                            branch,
                            is_active: true,
                            created_at: chrono::Utc::now(),
                            content: notice,
                            parent: Some(last_id),
                            token_count: None,
                        };
                        persist_intermediate(&event_sink, &notice_message).await?;
                        history.push(notice_message);
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
                    let supervisor_message = ChatMessage {
                        id: supervisor_id,
                        role: Role::Tool,
                        branch,
                        is_active: true,
                        created_at: chrono::Utc::now(),
                        content: directive,
                        parent: Some(last_id),
                        token_count: None,
                    };
                    persist_intermediate(&event_sink, &supervisor_message).await?;
                    history.push(supervisor_message);
                    last_id = supervisor_id;
                    tracing::warn!(tool = %blocked_tool, code = block_err.error_code(), "tool blocked — graceful degrade, LLM will answer from gathered context");
                }
                iteration += 1;
                continue;
            } else {
                // The bridge created the durable final assistant placeholder
                // before inference. Return its correct parent and token count
                // instead of minting a second, non-durable assistant id.
                return Ok(AgentRunOutcome {
                    final_parent: last_id,
                    final_token_count: Some(used_tokens as u32),
                    final_content: Some(assistant_text),
                    cancelled: false,
                });
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
    use crate::agent::context::{ContextBackend, TokenCount};
    use crate::agent::tools::FailureTracker;
    use crate::agent::watchdog::Watchdog;
    use crate::engine::BackendIdentity;
    use crate::tools::ToolRegistry;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Barrier;
    use std::time::Duration;
    use tokio::sync::Notify;

    struct StaticContextBackend;

    #[async_trait::async_trait]
    impl ContextBackend for StaticContextBackend {
        async fn load_history(
            &self,
            _conversation: ConversationId,
            _branch: BranchId,
            _active_history: &[ChatMessage],
        ) -> Result<Vec<ChatMessage>, MukeiError> {
            Ok(Vec::new())
        }

        async fn rag_lookup(&self, _query: &str, _top_k: usize) -> Result<Vec<String>, MukeiError> {
            Ok(Vec::new())
        }
    }

    struct FixedTokens;

    #[async_trait::async_trait]
    impl TokenCount for FixedTokens {
        async fn count(&self, text: &str) -> usize {
            text.len()
        }
    }

    struct PanicBackend;

    #[async_trait::async_trait]
    impl InferenceBackend for PanicBackend {
        fn identity(&self) -> BackendIdentity {
            BackendIdentity::production("panic_test_backend")
        }

        async fn run(
            &self,
            _prompt: &str,
            _cancel: CancellationToken,
            _token_sender: mpsc::Sender<String>,
        ) -> Result<InferenceOutcome, MukeiError> {
            panic!("synthetic inference panic")
        }
    }

    struct FailingBackend;

    #[async_trait::async_trait]
    impl InferenceBackend for FailingBackend {
        fn identity(&self) -> BackendIdentity {
            BackendIdentity::production("failing_test_backend")
        }

        async fn run(
            &self,
            _prompt: &str,
            _cancel: CancellationToken,
            _token_sender: mpsc::Sender<String>,
        ) -> Result<InferenceOutcome, MukeiError> {
            Err(MukeiError::ModelLoadFailed(
                "synthetic production backend failure".to_string(),
            ))
        }
    }

    struct SequencedBackend {
        calls: AtomicUsize,
        first_started: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl InferenceBackend for SequencedBackend {
        fn identity(&self) -> BackendIdentity {
            BackendIdentity::production("sequenced_test_backend")
        }

        async fn run(
            &self,
            _prompt: &str,
            _cancel: CancellationToken,
            _token_sender: mpsc::Sender<String>,
        ) -> Result<InferenceOutcome, MukeiError> {
            let call = self.calls.fetch_add(1, Ordering::AcqRel);
            if call == 0 {
                self.first_started.notify_one();
                tokio::time::sleep(Duration::from_millis(40)).await;
            } else {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            Ok(InferenceOutcome {
                assistant_text: format!("response-{call}"),
                used_tokens: 1,
                stop_reason: StopReason::Completed,
            })
        }
    }

    fn agent_parts() -> (ContextBudgetManager, ToolExecutor, WatchdogHandle) {
        let context = ContextBudgetManager::new(
            Arc::new(StaticContextBackend),
            Arc::new(FixedTokens),
            100_000,
        );
        let tools = ToolExecutor::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(FailureTracker::new()),
        );
        let watchdog = WatchdogHandle::new(Watchdog::new(4, 1_000_000, Duration::from_secs(30)));
        (context, tools, watchdog)
    }

    fn request() -> AgentRunRequest {
        let (token_sender, _token_receiver) = mpsc::channel(8);
        AgentRunRequest::new(
            "hello",
            ConversationId::new(),
            BranchId::new(),
            MessageId::new(),
            CancellationToken::new(),
            token_sender,
        )
    }

    #[test]
    fn agent_run_request_builder_preserves_durable_scope() {
        let conversation = ConversationId::new();
        let branch = BranchId::new();
        let user_message_id = MessageId::new();
        let cancel_token = CancellationToken::new();
        let (token_sender, _token_receiver) = mpsc::channel(4);

        let request = AgentRunRequest::new(
            "hello",
            conversation,
            branch,
            user_message_id,
            cancel_token.clone(),
            token_sender,
        );

        assert_eq!(request.user_input, "hello");
        assert_eq!(request.conversation, conversation);
        assert_eq!(request.branch, branch);
        assert_eq!(request.user_message_id, user_message_id);
        assert!(request.event_sink.is_none());
        assert!(!request.cancel_token.is_cancelled());
        cancel_token.cancel();
        assert!(request.cancel_token.is_cancelled());
    }

    #[test]
    fn production_compat_constructor_never_silently_selects_mock() {
        let (context, tools, watchdog) = agent_parts();
        let agent = AgentLoop::new(context, tools, watchdog);
        assert_eq!(agent.backend_kind(), BackendKind::Unavailable);
    }

    #[test]
    fn explicit_test_mock_remains_usable_and_identifiable() {
        let (context, tools, watchdog) = agent_parts();
        let agent = AgentLoop::new_with_mock_for_tests(context, tools, watchdog);
        assert_eq!(agent.backend_kind(), BackendKind::DevelopmentMock);
    }

    #[tokio::test]
    async fn backend_panic_becomes_terminal_failure_and_clears_active_run() {
        let (context, tools, watchdog) = agent_parts();
        let agent = AgentLoop::new_with_backend(context, tools, watchdog, Arc::new(PanicBackend));
        let result = agent.clone().run(request()).await;
        assert!(matches!(result, Err(MukeiError::Internal(_))));
        assert_eq!(agent.active_run_generation(), None);
    }

    #[tokio::test]
    async fn real_backend_failure_does_not_fall_back_to_mock() {
        let (context, tools, watchdog) = agent_parts();
        let agent = AgentLoop::new_with_backend(context, tools, watchdog, Arc::new(FailingBackend));
        let result = agent.clone().run(request()).await;
        assert!(matches!(result, Err(MukeiError::ModelLoadFailed(_))));
        assert_eq!(agent.backend_kind(), BackendKind::Production);
        assert_eq!(agent.active_run_generation(), None);
    }

    #[tokio::test]
    async fn cancellation_is_terminal_and_clears_active_run() {
        let (context, tools, watchdog) = agent_parts();
        let agent = AgentLoop::new_with_mock_for_tests(context, tools, watchdog);
        let request = request();
        request.cancel_token.cancel();
        let outcome = agent.clone().run(request).await.unwrap();
        assert!(outcome.cancelled);
        assert_eq!(agent.active_run_generation(), None);
    }

    #[tokio::test]
    async fn stale_completion_cannot_finish_a_newer_generation() {
        let (context, tools, watchdog) = agent_parts();
        let first_started = Arc::new(Notify::new());
        let backend = Arc::new(SequencedBackend {
            calls: AtomicUsize::new(0),
            first_started: first_started.clone(),
        });
        let agent = AgentLoop::new_with_backend(context, tools, watchdog, backend);

        let first_agent = agent.clone();
        let first = tokio::spawn(async move { first_agent.run(request()).await });
        first_started.notified().await;

        let second = agent.clone().run(request()).await.unwrap();
        assert_eq!(second.final_content.as_deref(), Some("response-1"));

        let stale = first.await.unwrap();
        assert!(
            matches!(stale, Err(MukeiError::Internal(message)) if message == "stale agent completion ignored")
        );
        assert_eq!(agent.active_run_generation(), None);
    }

    #[test]
    fn duplicate_terminal_completion_is_rejected() {
        let terminal = InferenceTerminalGuard::new();
        assert!(terminal.try_finish(InferenceTerminalOutcome::Failed));
        assert!(!terminal.try_finish(InferenceTerminalOutcome::Cancelled));
        assert!(!terminal.try_finish(InferenceTerminalOutcome::Succeeded));
    }

    #[test]
    fn cancel_and_failure_race_can_commit_only_one_terminal_result() {
        let terminal = Arc::new(InferenceTerminalGuard::new());
        let barrier = Arc::new(Barrier::new(3));

        let cancel_terminal = terminal.clone();
        let cancel_barrier = barrier.clone();
        let cancel = std::thread::spawn(move || {
            cancel_barrier.wait();
            cancel_terminal.try_finish(InferenceTerminalOutcome::Cancelled)
        });

        let failure_terminal = terminal.clone();
        let failure_barrier = barrier.clone();
        let failure = std::thread::spawn(move || {
            failure_barrier.wait();
            failure_terminal.try_finish(InferenceTerminalOutcome::Failed)
        });

        barrier.wait();
        let committed = cancel.join().unwrap() as usize + failure.join().unwrap() as usize;
        assert_eq!(committed, 1);
    }

    #[test]
    fn stale_run_guard_cannot_clear_newer_generation() {
        let active = AtomicU64::new(1);
        let stale = ActiveRunGuard {
            active_generation: &active,
            generation: 1,
        };
        active.store(2, Ordering::Release);
        drop(stale);
        assert_eq!(active.load(Ordering::Acquire), 2);
    }

    #[test]
    fn constants_match_default_config() {
        assert_eq!(DEFAULT_MAX_ITERATIONS, 8);
    }
}
