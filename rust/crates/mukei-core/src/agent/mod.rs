//! `mukei_core::agent` — TRD §2.
//!
//! Pure-Rust ReAct (Reason + Act) loop. The up-stream types from
//! [`crate::types`] and down-stream I/O from [`crate::engine`] and
//! [`crate::tools`] are *only* observable through this module's
//! public surface, which keeps the FFI boundary tight.

pub mod context;
pub mod loop_;
pub mod tools;
pub mod watchdog;

pub use context::{ContextBudget, ContextBudgetManager};
pub use loop_::{AgentLoop, AgentLoopHandle};
#[allow(deprecated)]
pub use tools::{
    FailureKind, FailureTracker, OutputRepeatTracker, ToolExecutionPolicy, ToolExecutor,
    ToolOutcome,
};
#[allow(deprecated)]
pub use tools::{ToolPolicy, MAX_FAILURES_PER_TOOL}; // legacy aliases
#[allow(deprecated)]
pub use tools::{render_repeat_output_envelope, render_tool_error_envelope};
pub use watchdog::{Watchdog, WatchdogHandle};

/// Global state machine snapshot. Mirrors `crate::ffi::agent`.
pub type AgentSnapshot = crate::ffi::agent::FfiAgentSnapshot;
