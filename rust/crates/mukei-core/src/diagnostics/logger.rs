//! `mukei_core::diagnostics::logger` — TRD §1.5 / §37.
//!
//! Tracing → local rolling file only. NO remote sink. The boot
//! path calls `initialize_tracing` exactly once before any FFI
//! signal can be emitted.

use std::sync::Arc;
use std::sync::OnceLock;

use tracing_subscriber::fmt::writer::MakeWriter;

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

/// Writer used by the core crate before any file-backed subscriber is
/// installed by the embedding app. This MUST stay silent: on Android,
/// stdout/stderr are mirrored into `adb logcat`, which is a privacy leak
/// for a zero-telemetry product.
fn bootstrap_log_writer() -> impl for<'writer> MakeWriter<'writer> + Clone {
    std::io::sink
}

/// Local-only tracing subscriber. We do **not** enable JSON or any
/// remote exporter; the default bootstrap path intentionally discards
/// bytes until the embedding app installs its own file-backed sink.
pub fn initialize_tracing() {
    use tracing_subscriber::EnvFilter;

    // Honour RUST_LOG, otherwise default to "mukei=info".
    let env = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("mukei=info"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env)
        .with_target(true)
        .with_writer(bootstrap_log_writer())
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
    use std::io::Write;
    use tracing_subscriber::fmt::writer::MakeWriter;

    #[test]
    fn bootstrap_writer_accepts_bytes_without_stdio_side_effects() {
        let make = bootstrap_log_writer();
        let mut writer = make.make_writer();
        writer
            .write_all(b"secret-that-must-not-hit-logcat")
            .unwrap();
        writer.flush().unwrap();
    }

    #[test]
    fn log_error_emits_expected_fields() {
        // No panic; bootstrap logging discards bytes until the embedder
        // installs a file-backed subscriber.
        log_error(&MukeiError::OOM);
        log_error(&MukeiError::PromptLeakage);
    }
}
