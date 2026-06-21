//! `mukei-core` — Agent / Engine / RAG / Storage / Diagnostics.
//!
//! Top-level cross-cuts:
//!   * Runs under the bounded `MukeiRuntime` (TRD §2.2 — Android uses
//!     `MAX_BLOCKING_THREADS=6` + `TOOL_BLOCKING_SLOTS=2`).
//!   * Every SQLite-bearing future is wrapped in `spawn_blocking` per the
//!     "Golden Rule" (TRD §2.4).
//!   * All callbacks leaving the Rust boundary are paired with a
//!     [`CallbackGuard`] (TRD §1.3, REQ-ARCH-05).
//!   * Crosses FFI only via the typed adapters in `crate::ffi`; this crate
//!     itself does not link to CXX-Qt so it can be unit-tested on any host.
//!
//! Public surface is intentionally narrow — QML only sees what is reachable
//! through `MukeiAgent` (bridge crate).

#![cfg_attr(
    all(not(test), feature = "release-hardening"),
    deny(unsafe_op_in_unsafe_fn)
)]
#![deny(rust_2018_idioms)]
// Gradual `missing_docs` re-enablement. Modules listed under
// `#[allow(missing_docs)]` below are pending a per-item documentation
// sweep; everything ELSE in this crate is required to carry doc-comments
// on every `pub` item.
//
// Adding a new pub item to one of the allow-listed modules is fine, but
// adding a new module here is NOT — every new module ships fully
// documented from day one.
#![warn(missing_docs)]

// ---------------------------------------------------------------------
// Crate-level feature plumbing.
// ---------------------------------------------------------------------
#[cfg(feature = "tokio")]
pub use tokio;

// ----- Public surface modules: missing_docs is ENFORCED ------------------
pub mod error;
pub mod guard;
pub mod ffi;

// ----- Crate-internal scaffolding: missing_docs allow-listed for now ----
// Each of these has a top-level `# Invariants` block; the per-item doc
// pass is tracked in the engineering backlog.
#[allow(missing_docs)] pub mod types;
#[cfg(feature = "tokio")]
#[allow(missing_docs)] pub mod runtime;
#[allow(missing_docs)] pub mod diagnostics;
#[allow(missing_docs)] pub mod agent;
#[allow(missing_docs)] pub mod engine;
#[allow(missing_docs)] pub mod rag;
#[allow(missing_docs)] pub mod tools;
#[allow(missing_docs)] pub mod storage;
#[allow(missing_docs)] pub mod config;

// Re-exports for ergonomic use from `mukei-bridge`.
pub use crate::error::{ErrorClass, MukeiError, Result};

// `cxx` is a hard dep upstream; here it is only a *trait* dep so we don't
// pull the entire C++ toolchain. The bridge crate re-exports the real
// handle.
pub mod prelude {
    //! Common imports for the core crates.
    pub use crate::error::{MukeiError, Result};
    pub use crate::{callback_with_guard, guard::{CallbackGuard, GuardError}};
    pub use crate::types::{
        ChatMessage, ConversationId, MessageId, Role, ToolCall, ToolCallId,
        ToolResult,
    };
}

#[cfg(doctest)]
/// Compile-time sanity check that `Result` is used everywhere.
#[allow(dead_code)]
fn _doc_assert_uses_result() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn error_is_send_and_sync() {
        fn assert_send<T: Send + Sync>() {}
        assert_send::<MukeiError>();
    }

    #[test]
    fn prelude_compiles() {
        let _ = prelude::CallbackGuard::invalid();
    }
}
