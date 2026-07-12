//! `mukei_core::diagnostics::panic_hook` — TRD §1.5 / REQ-ARCH-01.
//!
//! The hook is local-only, idempotent on normal installation, recursion-safe,
//! and deliberately redacts arbitrary panic payloads before persistence,
//! tracing or bridge callbacks.

use std::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};
#[cfg(test)]
use std::sync::Mutex;
use std::sync::{Arc, OnceLock};

use crate::diagnostics::crash_logger::{CrashFingerprint, CrashRecord};
use crate::diagnostics::logger;
use crate::diagnostics::redaction::{sanitize_stable_identifier, sanitize_telemetry_text};

static INSTALLED: OnceLock<()> = OnceLock::new();
static SINK: OnceLock<Arc<dyn PanicSink>> = OnceLock::new();

thread_local! {
    static IN_PANIC_HOOK: Cell<bool> = const { Cell::new(false) };
}

/// Trait the panic hook dispatches into. Implementations live in
/// `mukei-bridge` to translate panics into CXX-Qt error signals.
pub trait PanicSink: Send + Sync + std::panic::UnwindSafe {
    fn on_panic(&self, fingerprint: &CrashFingerprint, reason: &str);
}

/// Install the global panic hook exactly once. Repeated calls return without
/// replacing the first registered sink.
pub fn install_panic_hook(sink: Arc<dyn PanicSink>) {
    let _ = SINK.set(sink);
    if INSTALLED.set(()).is_err() {
        return;
    }
    std::panic::set_hook(Box::new(handle_panic));
}

pub fn is_installed() -> bool {
    INSTALLED.get().is_some()
}

/// Reclaim the process-global hook after a host framework overwrites it.
/// The first registered sink remains authoritative because `OnceLock` is used
/// intentionally to avoid a racing sink swap during panic handling.
pub fn reinstall_panic_hook(sink: Option<Arc<dyn PanicSink>>) {
    if let Some(new_sink) = sink {
        let _ = SINK.set(new_sink);
    }
    std::panic::set_hook(Box::new(handle_panic));
}

#[allow(clippy::incompatible_msrv)]
fn handle_panic(info: &std::panic::PanicHookInfo<'_>) {
    let recursive = IN_PANIC_HOOK.with(|flag| {
        if flag.get() {
            true
        } else {
            flag.set(true);
            false
        }
    });
    if recursive {
        return;
    }

    struct ResetGuard;
    impl Drop for ResetGuard {
        fn drop(&mut self) {
            IN_PANIC_HOOK.with(|flag| flag.set(false));
        }
    }
    let _reset = ResetGuard;

    let raw_location = info
        .location()
        .map(|location| format!("{}:{}", location.file(), location.line()))
        .unwrap_or_else(|| "unknown".to_string());
    let raw_reason = panic_payload_text(info.payload());

    // The fingerprint can safely include raw bytes because only the digest is
    // retained. Human-readable fields are redacted and bounded separately.
    let fingerprint = CrashFingerprint::from_panic(&raw_location, raw_reason);
    let safe_location = sanitize_telemetry_text(&raw_location, 192).into_string();
    let safe_reason = safe_panic_reason(raw_reason);
    let record = CrashRecord::new(
        fingerprint.clone(),
        safe_location.clone(),
        safe_reason.clone(),
    );

    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(crash_sink) = logger::crash_sink() {
            crash_sink.append(&record);
        }
    }));

    let _ = catch_unwind(AssertUnwindSafe(|| {
        tracing::error!(
            target = "mukei::panic",
            fingerprint = %fingerprint,
            location = %safe_location,
            reason = %safe_reason,
            "panic caught at FFI boundary"
        );
    }));

    if let Some(sink) = SINK.get().cloned() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            sink.on_panic(&fingerprint, &safe_reason);
        }));
    }
}

fn panic_payload_text(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(value) = payload.downcast_ref::<&'static str>() {
        value
    } else if let Some(value) = payload.downcast_ref::<String>() {
        value.as_str()
    } else {
        "non_string_panic"
    }
}

fn safe_panic_reason(reason: &str) -> String {
    let sanitized = sanitize_telemetry_text(reason, 192).into_string();
    sanitize_stable_identifier(&sanitized, 192).unwrap_or_else(|| "[redacted-content]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CapturingSink {
        hits: Mutex<Vec<(CrashFingerprint, String)>>,
    }

    impl PanicSink for CapturingSink {
        fn on_panic(&self, fingerprint: &CrashFingerprint, reason: &str) {
            self.hits
                .lock()
                .unwrap()
                .push((fingerprint.clone(), reason.into()));
        }
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
        let sink: Arc<dyn PanicSink> = Arc::new(CapturingSink {
            hits: Mutex::new(Vec::new()),
        });
        let _ = sink;
    }

    #[test]
    fn arbitrary_panic_content_is_redacted_and_bounded() {
        let raw = format!("user prompt with private content {}", "x".repeat(10_000));
        let safe = safe_panic_reason(&raw);
        assert_eq!(safe, "[redacted-content]");
        assert!(safe.len() <= 192);
    }

    #[test]
    fn stable_reason_code_is_preserved() {
        assert_eq!(
            safe_panic_reason("backend_unavailable"),
            "backend_unavailable"
        );
    }
    #[test]
    fn installation_is_idempotent() {
        let first: Arc<dyn PanicSink> = Arc::new(CapturingSink {
            hits: Mutex::new(Vec::new()),
        });
        let second: Arc<dyn PanicSink> = Arc::new(CapturingSink {
            hits: Mutex::new(Vec::new()),
        });
        install_panic_hook(first);
        install_panic_hook(second);
        assert!(is_installed());
    }
}
