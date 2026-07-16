//! Agent orchestration for context assembly, inference, tool execution, and
//! watchdog enforcement.
//!
//! Native transports submit commands through `application_runtime`; the agent
//! loop reports structured outcomes and streamed text to the runtime event bus.

pub mod context;
pub mod loop_;
pub mod tools;
pub mod watchdog;

pub use context::{ContextBudget, ContextBudgetManager, TokenCount};
pub use loop_::{
    AgentEventSink, AgentLoop, AgentLoopHandle, AgentRunOutcome, AgentRunRequest,
};
pub use tools::{
    FailureKind, FailureTracker, OutputRepeatTracker, ToolExecutionPolicy, ToolExecutor,
    ToolOutcome,
};
pub use tools::{render_repeat_output_envelope, render_tool_error_envelope};
pub use watchdog::{Watchdog, WatchdogHandle};

/// Platform-neutral runtime state snapshot retained for domain callers.
pub type AgentSnapshot = crate::boundary::RuntimeSnapshot;
