//! `mukei_core::diagnostics` — TRD §37.
//!
//! Two responsibilities, both absolutely-critical FMEA mitigations:
//!
//! 1. `panic_hook` — intercept every `unwind` at the FFI boundary so
//!    a `unwrap()` in Rust never tears down the whole Qt process (see
//!    REQ-ARCH-01 / §1.5 / PRD §4.2). Logs the trace locally; surfaces
//!    only the safe error class across the CXX-Qt bridge.
//!
//! 2. `crash_logger` — when an unrecoverable `MukeiError::CrashLoopDetected`
//!    fires (§36.1) the runtime writes a fingerprint to
//!    `/sdcard/Mukei/crashes/<sha256>.json` so the next boot can refuse
//!    to enter an infinite crash cycle.

pub mod crash_logger;
pub mod logger;
pub mod panic_hook;

pub use crash_logger::{CrashFingerprint, CrashRecord, CrashSink};
pub use logger::{initialize_tracing, log_error};
pub use panic_hook::{install_panic_hook, PanicSink};
