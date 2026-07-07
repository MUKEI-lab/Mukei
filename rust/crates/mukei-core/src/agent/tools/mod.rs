//! `mukei_core::agent::tools` — Tool Execution Pipeline (TRD §2.5 / PRD REQ-AGT-04).
//!
//! This module is the **runtime** half of tool calling. The leaf tool
//! implementations (web_search, file_tool, math, hardware) live under
//! [`crate::tools`]; everything in here is the orchestration layer that
//! dispatches them, classifies failures, hands structured feedback back
//! to the LLM, and detects no-progress loops.
//!
//! # Module layout
//!
//! ```text
//! agent/tools/
//! ├── mod.rs        ← this file (re-exports)
//! ├── policy.rs     ← ToolExecutionPolicy + FailureKind
//! ├── feedback.rs   ← StructuredFeedback envelope builders
//! ├── executor.rs   ← ToolExecutor (parallel dispatch + tracker)
//! └── watchdog.rs   ← Same-output / no-progress detection
//! ```
//!
//! # Invariants
//!
//! - **Threshold is configurable.** Default is
//!   [`ToolExecutionPolicy::DEFAULT_MAX_FAILURES`] (= 5). Production
//!   builds wire this from `config.toml::[agent]::max_failures_per_tool`.
//! - **Failure classes are distinguished.** See [`FailureKind`]: a
//!   `Cancelled` is NOT a failure (does not count toward the threshold);
//!   `Permanent` and `Abuse` block immediately regardless of threshold;
//!   `Transient` / `Validation` / `Timeout` count toward the threshold
//!   with distinct remediation hints.
//! - **The LLM receives typed feedback.** Every failed call is rendered
//!   as a `<external_data source="tool_error" kind="..." attempts="i/n">`
//!   block (see [`feedback`]) so the next turn has the metadata it needs.
//! - **No-progress detection.** When a tool returns byte-identical output
//!   for the same fingerprint `repeat_output_window` times in a row, the
//!   executor injects a backoff hint (see [`watchdog::OutputRepeatTracker`]).
//! - **Fingerprint is JSON-object-key-canonical**: `{a:1,b:2}` and
//!   `{b:2,a:1}` collide so re-ordering arguments cannot evade the blocker.

#![allow(missing_docs)] // per-item docs are exhaustive; suppress the umbrella warning

pub mod executor;
pub mod feedback;
pub mod policy;
pub mod watchdog;

// Public re-exports — agent/loop_.rs and the bridge crate consume these.
pub use executor::{FailureTracker, ToolExecutor, ToolOutcome};
pub use feedback::{
    render_repeat_output_envelope, render_supervisor_directive, render_tool_error_envelope,
};
pub use policy::{FailureKind, ToolExecutionPolicy};
pub use watchdog::OutputRepeatTracker;

// Issue #18: legacy `ToolPolicy` alias and `MAX_FAILURES_PER_TOOL`
// constant were deleted. The compile-time landmine (two constants with
// the same name and different values) is now gone. New code uses
// `ToolExecutionPolicy::max_failures_per_tool` directly.
