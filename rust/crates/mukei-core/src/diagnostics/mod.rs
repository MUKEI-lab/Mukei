//! `mukei_core::diagnostics` — TRD §37.
//!
//! Existing crash, panic, logger and redaction entry points remain available.
//! The `observability` module adds an owned, privacy-safe and bounded local
//! instrumentation foundation without introducing a remote telemetry backend.

pub mod crash_logger;
pub mod logger;
pub mod observability;
pub mod panic_hook;
pub mod redaction;

pub use crash_logger::{CrashFingerprint, CrashRecord, CrashSink};
pub use logger::{initialize_tracing, log_error};
pub use observability::*;
pub use panic_hook::{install_panic_hook, PanicSink};
pub use redaction::{
    redact_content, redact_path, redact_secret, sanitize_error_message, sanitize_log_value,
};
