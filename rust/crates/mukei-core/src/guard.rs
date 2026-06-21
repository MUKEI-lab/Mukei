//! `mukei_core::guard` — TRD §1.3, REQ-ARCH-05 (v0.6 CRITICAL fix).
//!
//! **`CallbackGuard` is the *only* FFI-safe callback lifetime tool the
//! workspace uses.** It eliminates the SIGSEGV tail of `extern "C" fn`
//! callbacks that survive Activity rotation / QObject destruction.
//!
//! \#  Design contract
//! ```text
//!   * `CallbackGuard` is *ABI-stable*: in the FFI struct it is a bare
//!     `u64`. Internally Rust overlays that pointer with an
//!     [`AtomicU64`] holding a monotonic generation number.
//!     We do **not** embed `AtomicU64` in the FFI struct — cxx-qt /
//!     manual-`extern "C"` both require POD layout there.
//!   * Every callback function `cb` is paired with:
//!       1. an opaque `*mut c_void`    (`context_ptr`),
//!       2. the caller-owned [`CallbackGuard`] (passed by pointer),
//!       3. the generation number at the moment the callback was bound.
//!   * Before invoking `cb`, the Rust caller:
//!       1. re-locates the guard by pointer,
//!       2. `load(Acquire)` the current generation,
//!       3. compares to the snapshot — if mismatch → **drop the call**.
//!   * `callback_with_guard!` wraps the call in `std::panic::catch_unwind`,
//!     guaranteeing no panic escapes across the FFI boundary.
//! ```
//!
//! REQ-ARCH-05 / §1.3.2 — every FFI callback crossing the bridge MUST be
//! paired with this guard AND wrapped in `catch_unwind`. The QML side
//! releases the guard (`mukei_release_callback_guard`) on destruction
//! so the dangling tail is provably impossible.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use thiserror::Error;

/// Errors produced by [`CallbackGuard`].
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    /// The guard has been released (or never existed) — the QObject
    /// should have been destroyed by now.
    #[error("callback guard has been released")]
    Released,
    /// The generation counter advanced — the original owner is stale.
    #[error("callback guard generation mismatch (expected {expected}, current {current})")]
    GenerationMismatch { expected: u64, current: u64 },
}

/// ABI-stable handle. On the FFI struct this is exposed as a bare `u64`
/// (REQ-ARCH-05 v0.7.2). 0 is the NULL-sentinel ("invalid guard").
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CallbackGuard(u64);

impl CallbackGuard {
    /// A pointer-typed constructor. The argument is the address of the
    /// Rust-side `Arc<Inner>` reduced to `usize`.
    ///
    /// # Safety
    /// `ptr` MUST be the address of a heap-allocated `Inner` that lives
    /// as long as the QObject it is paired with — else this is undefined
    /// behaviour. Used exclusively from `mukei-bridge` after handing out
    /// the typed handle from `unsafe extern "RustQt"` blocks.
    pub const unsafe fn from_ptr(ptr: usize) -> Self {
        Self(ptr as u64)
    }

    /// Sentinel "invalid" guard. Calling its `try_call` always yields
    /// `GuardError::Released`.
    pub const fn invalid() -> Self {
        Self(0)
    }

    /// Returns the raw pointer-sized value. Used for `Box::into_raw` /
    /// `Arc::into_raw` symmetry by the bridge crate.
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub fn is_valid(self) -> bool {
        self.0 != 0
    }

    /// Increment the generation counter on the *Rust* side, then drop
    /// the guard. The QObject observes the generation mismatch and stops
    /// firing callbacks immediately.
    ///
    /// # Safety
    /// `ptr_to_inner` MUST have been produced from `Arc::into_raw` of an
    /// `Inner`. Calling this more than once leaks.
    pub unsafe fn release(ptr_to_inner: *const Inner) {
        if !ptr_to_inner.is_null() {
            // Drop the Arc — the strong count goes to zero and `Inner`
            // is freed. The generation counter is no longer reachable
            // from QML, so every pending callback observes
            // `GuardError::Released` on its next call.
            drop(Arc::from_raw(ptr_to_inner));
        }
    }
}

/// The heap-allocated inner half of a [`CallbackGuard`]. Always reached
/// through `Arc<Inner>` — `release()` is what converts it back to raw.
pub struct Inner {
    /// Monotonic generation number. Starts at 1; the QObject binds its
    /// callback with a snapshot of "my" generation and rejects calls
    /// that read `current > snapshot`.
    pub generation: AtomicU64,
}

impl Inner {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { generation: AtomicU64::new(1) })
    }

    /// Increment generation. Idempotent on a freed Arc (caller checks
    /// first via the guard).
    pub fn invalidate(&self) {
        // u64::MAX means "this object has been destroyed". Any in-flight
        // callback whose snapshot != u64::MAX gets `GenerationMismatch`.
        self.generation.store(u64::MAX, Ordering::Release);
    }
}

/// Macro: invoke `callback` iff the guard's generation matches `snapshot`,
/// wrapping the body in `catch_unwind` so a panic becomes an `err` log
/// instead of an FFI crash.
///
/// # Arguments
/// - `$guard_ptr`  — expression evaluating to `*const Inner` (or
///                   `std::ptr::null()` if the guard is `invalid()`).
/// - `$snapshot`   — the generation number when the callback was bound.
/// - `$callback`   — block to invoke; must return `Result<T, E>`.
///
/// # Returns
/// - `Ok(T)` if generation matched AND callback returned `Ok`.
/// - `Err(GuardError)` if generation mismatched.
/// - `Err(GuardError::Panic)` if `catch_unwind` saw a panic.
#[macro_export]
macro_rules! callback_with_guard {
    ($guard_ptr:expr, $snapshot:expr, $callback:block) => {{
        // SAFETY: callers MUST uphold the contract documented on
        // [`CallbackGuard::from_ptr`]: `$guard_ptr` is a stable raw
        // pointer to a live `Inner` until release().
        let guard_ptr: *const $crate::guard::Inner = $guard_ptr;
        let snap: u64 = $snapshot;

        if guard_ptr.is_null() {
            Err($crate::guard::GuardError::Released)
        } else {
            let inner = unsafe { &*guard_ptr };
            let current = inner.generation.load(std::sync::atomic::Ordering::Acquire);
            if current != snap {
                Err($crate::guard::GuardError::GenerationMismatch {
                    expected: snap,
                    current,
                })
            } else {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || $callback)) {
                    Ok(result) => result,
                    Err(_) => Err($crate::guard::GuardError::Released), // any panic => drop
                }
            }
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    #[test]
    fn invalid_guard_rejects() {
        let err = callback_with_guard!(std::ptr::null(), 1u64, {
            Ok::<_, GuardError>(42)
        })
        .unwrap_err();
        assert_eq!(err, GuardError::Released);
    }

    #[test]
    fn valid_guard_passes() {
        let inner = Inner::new();
        let ptr = Arc::into_raw(Arc::clone(&inner));

        let snap = inner.generation.load(Ordering::Acquire);
        let result: Result<i32, GuardError> = callback_with_guard!(ptr, snap, {
            Ok::<_, GuardError>(7)
        });
        assert_eq!(result.unwrap(), 7);

        // Clean up.
        unsafe { CallbackGuard::release(ptr) };
    }

    #[test]
    fn invalidate_blocks_subsequent_calls() {
        let inner = Inner::new();
        let ptr = Arc::into_raw(Arc::clone(&inner));

        let stale_snap = inner.generation.load(Ordering::Acquire);
        inner.invalidate();

        let err = callback_with_guard!(ptr, stale_snap, {
            Ok::<_, GuardError>(0)
        })
        .unwrap_err();
        assert!(matches!(err, GuardError::GenerationMismatch { .. }));

        unsafe { CallbackGuard::release(ptr) };
    }

    #[test]
    fn panic_is_caught_not_propagated() {
        let inner = Inner::new();
        let ptr = Arc::into_raw(Arc::clone(&inner));
        let snap = inner.generation.load(Ordering::Acquire);

        let result: Result<i32, GuardError> = callback_with_guard!(ptr, snap, {
            panic!("intentional");
        });
        assert_eq!(result.unwrap_err(), GuardError::Released);

        unsafe { CallbackGuard::release(ptr) };
    }

    #[test]
    fn handle_is_u64_sized() {
        assert_eq!(std::mem::size_of::<CallbackGuard>(), 8);
    }
}
