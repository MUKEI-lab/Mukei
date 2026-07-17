//! `mukei-core` — platform-neutral agent, inference, RAG, storage, and runtime contracts.
//!
//! Android framework integration belongs in `mukei-android-jni`; presentation
//! state belongs in Kotlin/Compose. This crate contains neither UI toolkit nor
//! transport ownership.

#![cfg_attr(
    all(not(test), feature = "release-hardening"),
    deny(unsafe_op_in_unsafe_fn)
)]
#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

#[cfg(feature = "tokio")]
pub use tokio;

// Public, platform-neutral contracts.
#[cfg(feature = "tokio")]
#[allow(missing_docs)]
pub mod application_runtime;
pub mod boundary;
pub mod error;
pub mod guard;
#[cfg(feature = "tokio")]
pub mod platform;
pub mod saas;
pub mod ui_contract;
pub mod ui_protocol;

// Domain and infrastructure modules.
#[allow(missing_docs)]
pub mod agent;
#[allow(missing_docs)]
pub mod config;
#[allow(missing_docs)]
pub mod diagnostics;
#[allow(missing_docs)]
pub mod engine;
#[cfg(feature = "network")]
#[allow(missing_docs)]
pub mod network;
#[allow(missing_docs)]
pub mod rag;
#[cfg(feature = "tokio")]
#[allow(missing_docs)]
pub mod runtime;
#[allow(missing_docs)]
pub mod search;
#[allow(missing_docs)]
pub mod storage;
#[allow(missing_docs)]
pub mod tools;
#[allow(missing_docs)]
pub mod types;

pub use crate::error::{ErrorClass, MukeiError, Result};

/// Common imports for native transport crates.
pub mod prelude {
    #[cfg(feature = "tokio")]
    pub use crate::application_runtime::{
        EventDrain, MukeiRuntime, RuntimeConfig, RuntimeError, RuntimeServices,
        RuntimeSnapshotDomain, RuntimeSnapshotEnvelope, RuntimeState,
    };
    pub use crate::boundary::{
        BoundaryStateChange, LoadingStage, RuntimeSnapshot, StreamTagDetector, TagEvents,
    };
    pub use crate::error::{MukeiError, Result};
    pub use crate::guard::{BoundaryLease, GuardError, Inner as BoundaryLeaseOwner};
    #[cfg(feature = "tokio")]
    pub use crate::platform::{
        PlatformBrokerSnapshot, PlatformPortError, PlatformRequest, PlatformRequestBatch,
        PlatformRequestBroker, PlatformRequestKind, PlatformResponse, PlatformResponseStatus,
    };
    pub use crate::types::{
        ChatMessage, ConversationId, MessageId, Role, ToolCall, ToolCallId, ToolResult,
    };
}

#[cfg(doctest)]
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
    fn platform_boundary_compiles() {
        let lease = guard::BoundaryLease::invalid();
        assert!(!lease.is_valid());
        let mut detector = boundary::StreamTagDetector::new();
        assert!(detector.push("plain text").is_empty());
    }
}
