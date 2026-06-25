//! `mukei_core::runtime` — TRD §2.2.
//!
//! Provides the **single** bounded tokio runtime used by the entire
//! agent core. Two non-negotiable invariants live here:
//!
//! 1. **Android-bounded blocking pool** — `MAX_BLOCKING_THREADS=6` on
//!    `target_os = "android"`, 8 elsewhere. Mid-range Android (4–6 core
//!    SoCs) cannot tolerate the legacy 8-thread pool because SQLite writer
//!    + RAG indexer + tool executors starve the inference worker.
//!
//! 2. **Bounded tool semaphore** — `TOOL_BLOCKING_SLOTS=2` permits cap
//!    the number of concurrent `tool::spawn_blocking` evaluations to *2*,
//!    regardless of how many tools the LLM parallel-emits.  Inference
//!    is **never** counted against this budget — it lives on a dedicated
//!    worker (see `crate::engine::llama_wrapper`).
//!
//! Both invariants are surfaced as public statics so the rest of the
//! crate can `.acquire()` permits without ever creating a second pool.

use std::sync::Arc;
use std::time::Duration;

use once_cell::sync::Lazy;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Semaphore;

/// Android-bounded blocking pool size (TRD §2.2, v0.7.4 BUGFIX).
#[cfg(target_os = "android")]
pub const MAX_BLOCKING_THREADS: usize = 6;
/// Desktop / dev / CI default.
#[cfg(not(target_os = "android"))]
pub const MAX_BLOCKING_THREADS: usize = 8;

/// Number of concurrent `tool`-side `spawn_blocking` slots (TRD §2.2).
pub const TOOL_BLOCKING_SLOTS: usize = 2;

// Architect review GH #33 — compile-time invariant.
//
// `TOOL_BLOCKING_SLOTS` MUST stay strictly less than
// `MAX_BLOCKING_THREADS` on every target so a saturated tool semaphore
// can never starve every blocking-pool worker (inference, SQLite
// writer, RAG indexer). Enforced at build time — a future refactor
// that violates this ordering fails `cargo check`, not just CI.
const _: () = assert!(
    TOOL_BLOCKING_SLOTS < MAX_BLOCKING_THREADS,
    "TOOL_BLOCKING_SLOTS must be strictly less than MAX_BLOCKING_THREADS \
     (see TRD §2.2): otherwise saturating the tool semaphore would starve \
     the inference / DB / RAG workers."
);

/// Number of async worker threads. 4 is the sweet-spot for mobile — it
/// survives thermal-throttled 4-core SoCs without thrashing.
const WORKER_THREADS: usize = 4;

/// Global bounded tokio runtime. Initialised **once** on first access.
pub static RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    Builder::new_multi_thread()
        .worker_threads(WORKER_THREADS)
        .max_blocking_threads(MAX_BLOCKING_THREADS)
        .thread_name("mukei-tokio")
        .thread_keep_alive(Duration::from_secs(60))
        .enable_all()
        .build()
        .expect("Mukei runtime initialisation must not fail (mandatory invariant)")
});

/// Bounded slot for *all* tool-side `spawn_blocking` work.
///
/// Tools MUST `acquire_owned()` (or `acquire()`) one of these permits
/// before launching blocking work. This guarantees the inference worker
/// + DB writer always retain CPU / IO headroom.
pub static TOOL_SLOTS: Lazy<Arc<Semaphore>> =
    Lazy::new(|| Arc::new(Semaphore::new(TOOL_BLOCKING_SLOTS)));

/// Convenience helper: returns the global runtime reference.
///
/// # Panics
/// Panics if the runtime cannot be built — that is treated as a fatal
/// process-wide error because no Mukei code path can recover from a
/// scheduler failure.
pub fn get() -> &'static Runtime {
    &RUNTIME
}

/// Convenience helper: returns a clone of the `TOOL_SLOTS` semaphore arc
/// so the caller's `Arc<Semaphore>` clone lives as long as they do.
pub fn tool_slots() -> Arc<Semaphore> {
    Arc::clone(&TOOL_SLOTS)
}

/// Trampoline for spawn_blocking that *always* respects the v0.7.4
/// `MAX_BLOCKING_THREADS` invariant. Prefer over `tokio::task::spawn_blocking`
/// directly from inside `tools/*` so the cap cannot be accidentally
/// bypassed.
pub fn spawn_blocking_tool<F, T>(
    f: F,
) -> tokio::task::JoinHandle<std::result::Result<T, crate::error::MukeiError>>
where
    F: FnOnce() -> std::result::Result<T, crate::error::MukeiError> + Send + 'static,
    T: Send + 'static,
{
    let slots = tool_slots();
    RUNTIME.spawn(async move {
        // Blocking pool permits = the *runtime*'s blocking-pool permits.
        // TOOL_SLOTS is an additional soft cap above that, ensuring tool
        // work never *starves* the inference worker.
        let _permit = match slots.acquire_owned().await {
            Ok(p) => p,
            Err(_) => {
                return Err(crate::error::MukeiError::Internal(
                    "tool semaphore closed".into(),
                ))
            }
        };
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|e| crate::error::MukeiError::BlockingJoinFailed(e.to_string()))?
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_initialises_lazily() {
        // Touch the runtime; it must not panic.
        let _h = RUNTIME.spawn(async { 1u8 + 1 });
        // Test that the handle stays valid.
        std::mem::forget(_h);
    }

    #[test]
    fn tool_slots_default_is_two() {
        assert_eq!(TOOL_BLOCKING_SLOTS, 2);
    }

    #[test]
    fn blocking_thread_cap_is_target_specific() {
        #[cfg(target_os = "android")]
        assert_eq!(MAX_BLOCKING_THREADS, 6);
        #[cfg(not(target_os = "android"))]
        assert_eq!(MAX_BLOCKING_THREADS, 8);
    }

    /// Architect review GH #33 — confirm the cfg-gate that turns
    /// `MAX_BLOCKING_THREADS = 6` on Android compiles to a constant
    /// of 6, and that the desktop arm is exactly 8. This is a
    /// regression guard against a future refactor that accidentally
    /// swaps the arms or drops the `#[cfg(target_os = "android")]`.
    ///
    /// We exercise both arms simultaneously — the cfg-gate evaluates
    /// at compile time, so only one of these assertions is live for
    /// any given target build. The matrix `lint` job covers Linux
    /// (desktop arm); the Android NDK build that runs in the bridge
    /// crate covers the Android arm.
    #[test]
    fn blocking_thread_cap_matches_trd_section_2_2() {
        #[cfg(target_os = "android")]
        {
            assert_eq!(
                MAX_BLOCKING_THREADS, 6,
                "TRD §2.2: Android must cap blocking pool at 6 \
                 (4–6 core SoC + LMK headroom)."
            );
        }
        #[cfg(not(target_os = "android"))]
        {
            assert_eq!(
                MAX_BLOCKING_THREADS, 8,
                "TRD §2.2: desktop / CI default must stay at 8 \
                 (matches the original tokio default before the v0.7.4 \
                 Android tightening)."
            );
        }
        // The tool-semaphore vs. blocking-pool ordering check is a
        // compile-time invariant; the clippy lint
        // `assertions_on_constants` (correctly) flags a runtime
        // `assert!` on two `const usize` operands as redundant. The
        // module-level `const _: () = assert!(..)` next to the
        // `TOOL_BLOCKING_SLOTS` definition enforces the same property
        // at build time, which is strictly stronger than a runtime
        // test.
    }

    #[test]
    fn tool_slots_arc_can_be_cloned() {
        let a = tool_slots();
        let b = a.clone();
        assert!(Arc::ptr_eq(&a, &b) || std::sync::Arc::strong_count(&a) >= 2);
    }
}
