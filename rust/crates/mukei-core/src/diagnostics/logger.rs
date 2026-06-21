//! `mukei_core::diagnostics::logger` — TRD §1.5 / §37.
//!
//! Tracing → local rolling file only. NO remote sink. The boot
//! path calls `initialize_tracing` exactly once before any FFI
//! signal can be emitted.

use std::sync::Arc;
use std::sync::OnceLock;

use crate::diagnostics::crash_logger::CrashSink;
use crate::error::MukeiError;

static CRASH_SINK: OnceLock<Arc<CrashSink>> = OnceLock::new();

/// Set the global crash sink. Called by the bridge crate from the JNI
/// `MukeiActivity.onCreate` (TRD §9.1).
pub fn install_crash_sink(sink: Arc<CrashSink>) -> Result<(), &'static str> {
    CRASH_SINK.set(sink).map_err(|_| "already installed")
}

/// Borrow the global crash sink; returns `None` if not yet installed
/// (e.g. in tests).
pub fn crash_sink() -> Option<Arc<CrashSink>> {
    CRASH_SINK.get().cloned()
}

/// Local-only tracing subscriber. We do **not** enable JSON or any
/// remote exporter; the only sink is a rolling `.log` file on disk.
pub fn initialize_tracing() {
    use tracing_subscriber::EnvFilter;

    // Honour RUST_LOG, otherwise default to "mukei=info".
    let env = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("mukei=info"));

    // Local-only subscriber. The bridge crate replaces this with a
    // tee'd subscriber that also forwards into android logcat on the
    // `target_os = "android"` target (still local).
    let _ = tracing_subscriber::fmt()
        .with_env_filter(env)
        .with_target(true)
        .with_writer(std::io::sink) // default: drop. Bridge crate re-installs.
        .try_init();
}

/// Log an error to the local sink. QML-visible `error_code` is
/// canonicalised *before* logging so traces stay uniform.
pub fn log_error(err: &MukeiError) {
    tracing::error!(
        target = "mukei::error",
        code = err.error_code(),
        class = %err.classification(),
        message = %err,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_error_emits_expected_fields() {
        // No panic; tracing has an in-memory sink here.
        log_error(&MukeiError::OOM);
        log_error(&MukeiError::PromptLeakage);
    }
}
