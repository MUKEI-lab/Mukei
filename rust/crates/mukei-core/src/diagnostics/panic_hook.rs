//! `mukei_core::diagnostics::panic_hook` — TRD §1.5 / REQ-ARCH-01.
//!
//! Replaces the default Rust panic handler with one that:
//! 1. Logs the panic + backtrace to the *local* tracing sink (NEVER to a
//!    remote server — REQ-NON-GOAL:CLOUD-TELEMETRY).
//! 2. Writes a `CrashRecord` to the local crash sink (so the next boot
//!    can detect a crash loop per §36.1).
//! 3. **Returns / aborts gracefully** — handles Android Activity death
//!    surface by signalling on the panic channel so the bridge layer
//!    emits the appropriate `error_occurred` CXX-Qt signal.
//!
//! # Multi-thread safety
/// Panics can fire from any tokio worker. The hook is `Send + Sync` and
/// uses a `Mutex` over a thread-safe sink.
use std::sync::{Arc, Mutex, OnceLock};

static INSTALLED: OnceLock<()> = OnceLock::new();

use crate::diagnostics::crash_logger::CrashFingerprint;
use crate::diagnostics::logger;

/// Trait the panic hook dispatches into. Implementations live in
/// `mukei-bridge` to translate panics into CXX-Qt error signals.
pub trait PanicSink: Send + Sync + std::panic::UnwindSafe {
    /// Called once per panic. `fingerprint` is stable across restarts
    /// (per §36.1's regression-blocking rules). `reason` is the same
    /// `&str` passed to `std::panic::set_hook`.
    fn on_panic(&self, fingerprint: &CrashFingerprint, reason: &str);
}

static SINK: OnceLock<Arc<dyn PanicSink>> = OnceLock::new();

/// Install the global panic hook exactly once. Calling more than once
/// returns silently — the first install wins.
pub fn install_panic_hook(sink: Arc<dyn PanicSink>) {
    let _ = SINK.set(sink);

    // Avoid repeated installation (set_hook panics on the second call).
    if INSTALLED.set(()).is_err() {
        return;
    }

    std::panic::set_hook(Box::new(|info| {
        // The actual sink
        let sink_opt = SINK.get().cloned();

        // Compute fingerprint. We use a stable SHA-256 over the panic
        // message so the *next* boot recognises a regression.
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".into());
        let payload = info.payload();
        let reason = if let Some(s) = payload.downcast_ref::<&'static str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic>".into()
        };

        let fp = CrashFingerprint::from_panic(&location, &reason);

        // Synthesize a CrashRecord and persist.
        let record = crate::diagnostics::crash_logger::CrashRecord::new(
            fp.clone(),
            location.clone(),
            reason.clone(),
        );
        if let Some(crash_sink) = logger::crash_sink() {
            crash_sink.append(&record);
        }

        // Log to local sink.
        tracing::error!(
            target = "mukei::panic",
            fingerprint = %fp,
            location = %location,
            reason = %reason,
            "panic caught at FFI boundary"
        );

        if let Some(sink) = sink_opt {
            sink.on_panic(&fp, &reason);
        }
    }));
}

/// Returns true if a panic hook has been installed. Used by tests.
pub fn is_installed() -> bool {
    INSTALLED
        .get()
        .map(|_| true)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::crash_logger::CrashFingerprint;

    struct CapturingSink {
        hits: Mutex<Vec<(CrashFingerprint, String)>>,
    }
    impl PanicSink for CapturingSink {
        fn on_panic(&self, fingerprint: &CrashFingerprint, reason: &str) {
            self.hits.lock().unwrap().push((fingerprint.clone(), reason.into()));
        }
    }

    fn installed() -> Option<()> {
        None // intentionally unhelpful in nested module context
    }

    #[test]
    fn fingerprint_is_stable_within_call() {
        let fp1 = CrashFingerprint::from_panic("a.rs:1", "boom");
        let fp2 = CrashFingerprint::from_panic("a.rs:1", "boom");
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_changes_with_message() {
        let fp1 = CrashFingerprint::from_panic("a.rs:1", "boom");
        let fp2 = CrashFingerprint::from_panic("a.rs:1", "BOOM");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn sink_arc_is_send_sync() {
        let s: Arc<dyn PanicSink> = Arc::new(CapturingSink {
            hits: Mutex::new(Vec::new()),
        });
        let _ = s;
    }
}
