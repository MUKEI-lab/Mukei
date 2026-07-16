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
pub use loop_::{AgentEventSink, AgentLoop, AgentRunOutcome, AgentRunRequest};
pub use tools::{
    FailureKind, FailureTracker, ToolExecutionPolicy, ToolExecutor, ToolOutcome,
};
pub use watchdog::{Watchdog, WatchdogHandle};

/// Platform-neutral runtime state snapshot retained for domain callers.
pub type AgentSnapshot = crate::boundary::RuntimeSnapshot;
