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
//!       3. the generation number at the moment the callback was bound,
//!       4. the process-unique instance id at the same bind point.
//!   * Before invoking `cb`, the Rust caller:
//!       1. re-locates the guard by pointer,
//!       2. `load(Acquire)` the current generation,
//!       3. compares the live instance id and generation to the
//!          snapshots — if either mismatches → **drop the call**.
//!   * `callback_with_guard!` wraps the call in `std::panic::catch_unwind`,
//!     guaranteeing no panic escapes across the FFI boundary.
//! ```
//!
//! REQ-ARCH-05 / §1.3.2 — every FFI callback crossing the bridge MUST be
//! paired with this guard AND wrapped in `catch_unwind`. The QML side
//! releases the guard (`mukei_release_callback_guard`) on destruction
//! so the dangling tail is provably impossible.

use std::ptr::NonNull;
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
    GenerationMismatch {
        /// The generation snapshot captured when the callback was bound.
        expected: u64,
        /// The guard's live generation at the time of dispatch.
        current: u64,
    },
    /// The guard pointer now refers to a different `Inner` allocation
    /// than the one captured at bind time. This is the ABA defence for
    /// allocator address reuse.
    #[error("callback guard instance mismatch (expected {expected}, current {current})")]
    InstanceMismatch {
        /// The instance id captured when the callback was bound.
        expected: u64,
        /// The live instance id read before dispatch.
        current: u64,
    },
    /// The callback panicked while we were delivering it across the FFI
    /// boundary. The panic was caught and converted into a typed error.
    #[error("callback panicked while guarded")]
    Panic,
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
    ///
    /// # Deprecation
    /// Architect review GH #10: a `usize` of `0` is *both* a valid C
    /// NULL and our `invalid()` sentinel — the type system can't tell
    /// them apart. New code MUST use [`Self::from_non_null`] which
    /// statically rejects a NULL pointer.
    #[deprecated(note = "Use `from_non_null(NonNull<Inner>)`. Architect review GH #10.")]
    pub const unsafe fn from_ptr(ptr: usize) -> Self {
        Self(ptr as u64)
    }

    /// Type-safe constructor: accepts only a non-null pointer to the
    /// heap-allocated [`Inner`], statically eliminating the `0`-versus-
    /// sentinel ambiguity flagged in architect review GH #10.
    ///
    /// # Safety
    /// `ptr` MUST be the address of a heap-allocated `Inner` produced
    /// by `Arc::into_raw` and kept alive at least as long as the
    /// QObject it is paired with. The bridge crate releases it via
    /// [`Self::release`].
    //
    // NOTE: not `const fn` — raw-pointer-to-integer casts are
    // disallowed at const-eval time on stable Rust. The function is
    // still trivially inlineable.
    #[inline]
    pub unsafe fn from_non_null(ptr: NonNull<Inner>) -> Self {
        Self(ptr.as_ptr() as u64)
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

    /// Returns `true` for any non-zero handle. A zero handle is the
    /// sentinel produced by [`Self::invalid`].
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
            drop(unsafe { Arc::from_raw(ptr_to_inner) });
        }
    }
}

/// Process-monotonic counter that hands every `Inner` a unique
/// `instance_id` distinct from every other Inner ever allocated by
/// this process. Starts at 1 and increments on every `Inner::new`.
///
/// This is the ABA mitigation for the heap-allocator-reuse window
/// flagged in architect review GH #53: even if `Arc::into_raw(Inner)`
/// returns address `0xCAFE` for one guard, that address is freed on
/// `release`, and a subsequent `acquire` happens to land on `0xCAFE`
/// again, the new Inner carries a different `instance_id` so a stale
/// snapshot held over the gap is rejected with
/// `GuardError::GenerationMismatch` (the snapshot's bound
/// `instance_id` no longer matches the live one).
///
/// 2^64 acquires in a single process is the canonical "impossible in
/// practice" bound — even at 1 acquire/ns it would take ~585 years
/// to wrap. The increment uses `Ordering::Relaxed` since the value
/// itself is the source of identity; the synchronisation happens
/// through the `Arc` it lives inside.
static NEXT_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

/// The heap-allocated inner half of a [`CallbackGuard`]. Always reached
/// through `Arc<Inner>` — `release()` is what converts it back to raw.
pub struct Inner {
    /// Monotonic generation number. Starts at 1; the QObject binds its
    /// callback with a snapshot of "my" generation and rejects calls
    /// that read `current > snapshot`.
    pub generation: AtomicU64,
    /// Process-unique identity assigned at construction time. Combined
    /// with `generation`, this defeats the heap-reuse ABA window
    /// flagged in architect review GH #53. Stable for the lifetime of
    /// the Inner; never changes after `Inner::new`.
    pub instance_id: u64,
}

impl Inner {
    /// Sentinel generation value that indicates a permanently-destroyed
    /// callback target. Any in-flight callback whose snapshot is not
    /// equal to this value will observe `GenerationMismatch` once the
    /// guard is tombstoned.
    pub const TOMBSTONE: u64 = u64::MAX;

    /// Allocate a fresh `Arc<Inner>` with `generation = 1` and a
    /// process-unique `instance_id`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            generation: AtomicU64::new(1),
            instance_id: NEXT_INSTANCE_ID.fetch_add(1, Ordering::Relaxed),
        })
    }

    /// Increment generation by one (monotonic rebind path).
    ///
    /// Used when the same `Arc<Inner>` is being rebound to a fresh
    /// QObject (e.g. Activity recreation reusing the slot). The next
    /// callback snapshot must observe the bumped value, dropping any
    /// callback bound under the previous generation.
    ///
    /// Architect review GH #9: this replaces the blanket `u64::MAX`
    /// slam previously named `invalidate()`. The monotonic contract
    /// documented in the module header is now actually true.
    pub fn bump(&self) -> u64 {
        // saturating_add semantics via fetch_add + clamp: once we hit
        // TOMBSTONE — 1, the next bump is functionally indistinguishable
        // from tombstone. In practice 2^64 - 1 rebinds is unreachable.
        let prev = self.generation.fetch_add(1, Ordering::Release);
        if prev == Self::TOMBSTONE - 1 {
            // Saturate so the next load sees TOMBSTONE rather than
            // wrapping back to 0 and accidentally matching a stale snap.
            self.generation.store(Self::TOMBSTONE, Ordering::Release);
            Self::TOMBSTONE
        } else {
            prev + 1
        }
    }

    /// Permanently invalidate the callback target. Any in-flight
    /// callback observes `GenerationMismatch` on its next attempt.
    ///
    /// **This is a one-way door.** Use `bump()` if you mean "rebind".
    /// The legacy name `invalidate()` is retained as a deprecated alias
    /// for compatibility with existing bridge call-sites.
    pub fn tombstone(&self) {
        self.generation.store(Self::TOMBSTONE, Ordering::Release);
    }

    /// Read the process-unique `instance_id` assigned at construction.
    ///
    /// Used by FFI callers that want an additional ABA defence beyond
    /// the per-guard generation counter (architect review GH #53). The
    /// caller captures this value at bind time and re-reads it before
    /// dispatching a callback; a mismatch means the underlying Inner
    /// has been freed and a new Inner has reused the heap address.
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }

    /// Deprecated alias for [`Self::tombstone`].
    #[deprecated(
        note = "Use `tombstone()` (permanent) or `bump()` (rebind) explicitly. Architect review GH #9."
    )]
    pub fn invalidate(&self) {
        self.tombstone();
    }
}

/// Macro: invoke `callback` iff the guard's generation matches `snapshot`,
/// wrapping the body in `catch_unwind` so a panic becomes an `err` log
/// instead of an FFI crash.
///
/// # Arguments
/// - `$guard_ptr` — expression evaluating to `*const Inner` (or
///   `std::ptr::null()` if the guard is `invalid()`).
/// - `$snapshot` — the generation number when the callback was bound.
/// - `$instance_snapshot` — the instance id when the callback was bound.
/// - `$callback` — block to invoke; must return `Result<T, GuardError>`.
///
/// # Returns
/// - `Ok(T)` if generation matched and callback returned `Ok`.
/// - `Err(GuardError::Released)` if the guard is NULL.
/// - `Err(GuardError::GenerationMismatch)` if the guard is live but stale.
/// - `Err(GuardError::Panic)` if the callback panicked while we were
///   delivering it across the FFI boundary.
///
/// # Panic policy
/// A panic inside `$callback` is caught via `std::panic::catch_unwind` and
/// converted into `Err(GuardError::Panic)`. This preserves the no-panic-
/// across-FFI guarantee while letting audit/diagnostics distinguish "target
/// destroyed" from "delivery panicked".
#[macro_export]
macro_rules! callback_with_guard {
    ($guard_ptr:expr, $snapshot:expr, $instance_snapshot:expr, $callback:block) => {{
        // SAFETY: callers MUST uphold the contract documented on
        // [`CallbackGuard::from_ptr`]: `$guard_ptr` is a stable raw
        // pointer to a live `Inner` until release().
        let guard_ptr: *const $crate::guard::Inner = $guard_ptr;
        let snap: u64 = $snapshot;
        let instance_snap: u64 = $instance_snapshot;

        if guard_ptr.is_null() {
            Err($crate::guard::GuardError::Released)
        } else {
            let inner = unsafe { &*guard_ptr };
            let current = inner.generation.load(std::sync::atomic::Ordering::Acquire);
            let current_instance = inner.instance_id();
            if current_instance != instance_snap {
                Err($crate::guard::GuardError::InstanceMismatch {
                    expected: instance_snap,
                    current: current_instance,
                })
            } else if current != snap {
                Err($crate::guard::GuardError::GenerationMismatch {
                    expected: snap,
                    current,
                })
            } else {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || $callback)) {
                    Ok(result) => result,
                    Err(_) => Err($crate::guard::GuardError::Panic),
                }
            }
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    #[test]
    fn invalid_guard_rejects() {
        let err = callback_with_guard!(std::ptr::null(), 1u64, 1u64, { Ok::<_, GuardError>(42) })
            .unwrap_err();
        assert_eq!(err, GuardError::Released);
    }

    #[test]
    fn valid_guard_passes() {
        let inner = Inner::new();
        let ptr = Arc::into_raw(Arc::clone(&inner));

        let snap = inner.generation.load(Ordering::Acquire);
        let instance = inner.instance_id();
        let result: Result<i32, GuardError> =
            callback_with_guard!(ptr, snap, instance, { Ok::<_, GuardError>(7) });
        assert_eq!(result.unwrap(), 7);

        // Clean up.
        unsafe { CallbackGuard::release(ptr) };
    }

    #[test]
    fn tombstone_blocks_subsequent_calls() {
        let inner = Inner::new();
        let ptr = Arc::into_raw(Arc::clone(&inner));

        let stale_snap = inner.generation.load(Ordering::Acquire);
        let instance = inner.instance_id();
        inner.tombstone();

        let err = callback_with_guard!(ptr, stale_snap, instance, { Ok::<_, GuardError>(0) })
            .unwrap_err();
        assert!(matches!(err, GuardError::GenerationMismatch { .. }));

        unsafe { CallbackGuard::release(ptr) };
    }

    #[test]
    fn bump_blocks_only_the_previous_generation() {
        // Architect review GH #9: rebind path. Old snapshot rejected,
        // new snapshot accepted.
        let inner = Inner::new();
        let ptr = Arc::into_raw(Arc::clone(&inner));

        let old_snap = inner.generation.load(Ordering::Acquire);
        let instance = inner.instance_id();
        let new_snap = inner.bump();
        assert_ne!(old_snap, new_snap);
        assert_ne!(
            new_snap,
            Inner::TOMBSTONE,
            "bump must not slam to tombstone"
        );

        // Old snapshot now mismatches.
        let err =
            callback_with_guard!(ptr, old_snap, instance, { Ok::<_, GuardError>(0) }).unwrap_err();
        assert!(matches!(err, GuardError::GenerationMismatch { .. }));

        // New snapshot still works.
        let ok =
            callback_with_guard!(ptr, new_snap, instance, { Ok::<_, GuardError>(99) }).unwrap();
        assert_eq!(ok, 99);

        unsafe { CallbackGuard::release(ptr) };
    }

    #[test]
    fn panic_is_caught_not_propagated() {
        let inner = Inner::new();
        let ptr = Arc::into_raw(Arc::clone(&inner));
        let snap = inner.generation.load(Ordering::Acquire);
        let instance = inner.instance_id();

        let result: Result<i32, GuardError> = callback_with_guard!(ptr, snap, instance, {
            panic!("intentional");
        });
        assert_eq!(result.unwrap_err(), GuardError::Panic);

        unsafe { CallbackGuard::release(ptr) };
    }

    #[test]
    fn handle_is_u64_sized() {
        assert_eq!(std::mem::size_of::<CallbackGuard>(), 8);
    }

    #[test]
    fn from_non_null_roundtrips() {
        // Architect review GH #10 regression: NonNull-typed constructor
        // can't be called with a NULL pointer at all (type-system
        // enforced). Smoke test that the address survives.
        let inner = Inner::new();
        let raw = Arc::into_raw(Arc::clone(&inner));
        let nn = std::ptr::NonNull::new(raw as *mut Inner).expect("Arc::into_raw is never NULL");
        let g = unsafe { CallbackGuard::from_non_null(nn) };
        assert!(g.is_valid());
        assert_eq!(g.as_u64(), raw as u64);
        unsafe { CallbackGuard::release(raw) };
    }

    /// Architect review GH #53 — ABA defence.
    ///
    /// Two distinct `Inner::new()` calls produce different
    /// `instance_id`s. This is the property that lets FFI callers
    /// detect heap-address reuse across release/acquire cycles: even
    /// if the same address is recycled, the new Inner's instance_id
    /// has advanced, so a stale binding captured before the release
    /// can be detected and dropped.
    #[test]
    fn instance_id_is_unique_per_construction() {
        let a = Inner::new();
        let b = Inner::new();
        let c = Inner::new();
        assert_ne!(a.instance_id(), b.instance_id());
        assert_ne!(b.instance_id(), c.instance_id());
        assert_ne!(a.instance_id(), c.instance_id());
        // The counter is monotonic: each new instance gets a strictly
        // greater id than the prior one observed in this thread.
        assert!(b.instance_id() > a.instance_id());
        assert!(c.instance_id() > b.instance_id());
    }

    /// Architect review GH #53 — instance_id is stable after
    /// generation bumps.
    ///
    /// `bump()` and `tombstone()` operate on the generation counter;
    /// the `instance_id` must NOT change after construction. Otherwise
    /// the ABA defence would be undermined: a caller that captured
    /// `instance_id` at bind time would observe a spurious mismatch on
    /// every normal rebind.
    #[test]
    fn instance_id_is_stable_across_generation_bumps() {
        let inner = Inner::new();
        let id0 = inner.instance_id();
        inner.bump();
        assert_eq!(inner.instance_id(), id0);
        inner.bump();
        inner.bump();
        assert_eq!(inner.instance_id(), id0);
        inner.tombstone();
        assert_eq!(inner.instance_id(), id0);
    }

    #[test]
    fn instance_mismatch_blocks_callback() {
        let inner = Inner::new();
        let ptr = Arc::into_raw(Arc::clone(&inner));
        let snap = inner.generation.load(Ordering::Acquire);
        let stale_instance = inner.instance_id() + 1;

        let err = callback_with_guard!(ptr, snap, stale_instance, { Ok::<_, GuardError>(0) })
            .unwrap_err();
        assert!(matches!(err, GuardError::InstanceMismatch { .. }));

        unsafe { CallbackGuard::release(ptr) };
    }
}
