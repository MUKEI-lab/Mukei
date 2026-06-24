//! Manual C-FFI escape hatch — TRD §1.3.2.
//!
//! This crate is the **only** place in the workspace that exposes a stable
//! `extern "C"` surface to Qt/QML for the manual fallback path. Every
//! callback that crosses this boundary is paired with a
//! [`mukei_core::guard::CallbackGuard`] and dispatched through the
//! [`mukei_core::callback_with_guard!`] macro so that:
//!
//!  1. A destroyed QObject can never receive a tail-callback (generation
//!     mismatch → drop).
//!  2. A Rust-side panic can never escape across the FFI boundary
//!     (`std::panic::catch_unwind` wrapper inside the macro).
//!
//! Note: workspace-wide `panic = "unwind"` is REQUIRED (TRD §1.3 / PRD G1).
//! `catch_unwind` is a no-op under `panic = "abort"`, which would defeat
//! every guarantee above.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use mukei_core::callback_with_guard;
use mukei_core::guard::{GuardError, Inner};

/// Opaque ABI handle as seen from C/Qt/QML. The pointer aliases an
/// `Arc<Inner>` heap allocation owned by Rust. C code never dereferences
/// it — it is only ever round-tripped back into this crate.
#[repr(transparent)]
pub struct CallbackGuardHandle(*const Inner);

/// FFI token callback signature.
///
/// Arguments:
///   * `context_ptr` — opaque pointer the QObject side passed at bind.
///   * `generation`  — the guard's generation at the time of the call.
///   * `token`       — UTF-8, NUL-terminated chunk of streamed text.
pub type TokenCallback =
    extern "C" fn(context_ptr: *mut c_void, generation: u64, token: *const c_char);

// ---------------------------------------------------------------------
// Guard lifecycle  — TRD §1.3.2
// ---------------------------------------------------------------------

/// Allocate a fresh callback guard. Returned pointer is non-null on
/// success and MUST be released exactly once via
/// [`mukei_release_callback_guard`] after every paired callback path is
/// either delivered or generation-bumped.
#[no_mangle]
pub extern "C" fn mukei_acquire_callback_guard() -> *const Inner {
    Arc::into_raw(Inner::new())
}

/// Release a guard. After this call the `guard_ptr` is invalid; all
/// in-flight callbacks observe `GuardError::Released` and drop.
#[no_mangle]
pub extern "C" fn mukei_release_callback_guard(guard_ptr: *const Inner) {
    if guard_ptr.is_null() {
        return;
    }
    // SAFETY: `guard_ptr` was produced by `Arc::into_raw` in
    // `mukei_acquire_callback_guard`. The caller's contract is to call
    // this exactly once.
    unsafe { mukei_core::guard::CallbackGuard::release(guard_ptr) };
}

/// Read the current generation counter. Used by the C side when it wants
/// to bind a callback against the current guard state.
#[no_mangle]
pub extern "C" fn mukei_callback_guard_current_generation(guard_ptr: *const Inner) -> u64 {
    if guard_ptr.is_null() {
        return 0;
    }
    // SAFETY: pointer was produced by `mukei_acquire_callback_guard` and
    // has not been released yet (caller's contract).
    let inner = unsafe { &*guard_ptr };
    inner.generation.load(Ordering::Acquire)
}

/// Atomically bump the generation. Returns the **new** generation
/// number, or 0 on a NULL guard. Equivalent to "logically cancel every
/// in-flight callback bound against the previous generation".
#[no_mangle]
pub extern "C" fn mukei_callback_guard_bump_generation(guard_ptr: *const Inner) -> u64 {
    if guard_ptr.is_null() {
        return 0;
    }
    // SAFETY: see above.
    let inner = unsafe { &*guard_ptr };
    inner.generation.fetch_add(1, Ordering::AcqRel) + 1
}

/// True iff `generation` matches the guard's current generation.
#[no_mangle]
pub extern "C" fn mukei_callback_guard_matches(guard_ptr: *const Inner, generation: u64) -> bool {
    if guard_ptr.is_null() {
        return false;
    }
    // SAFETY: see above.
    let inner = unsafe { &*guard_ptr };
    inner.generation.load(Ordering::Acquire) == generation
}

/// Bump the guard's generation as the "stop the world" signal. Any
/// callback still scheduled against the previous generation will be
/// dropped on its next dispatch.
#[no_mangle]
pub extern "C" fn mukei_stop_generation(guard_ptr: *const Inner) {
    let _ = mukei_callback_guard_bump_generation(guard_ptr);
}

// ---------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn mukei_initialize(_config_path: *const c_char) -> bool {
    // Real boot lives in mukei-bridge; the shim only confirms the entry
    // point is wired so QML can probe for symbol availability.
    true
}

// ---------------------------------------------------------------------
// Streamed send_message — fully guard-protected
// ---------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn mukei_send_message(
    user_input: *const c_char,
    context_ptr: *mut c_void,
    guard_ptr: *const Inner,
    callback: TokenCallback,
) -> u64 {
    if user_input.is_null() || context_ptr.is_null() || guard_ptr.is_null() {
        return 0;
    }

    // SAFETY: callers guarantee `user_input` is a valid UTF-8 C string.
    let input = match unsafe { CStr::from_ptr(user_input) }.to_str() {
        Ok(value) => value.to_owned(),
        Err(_) => return 0,
    };

    // Bind the callback against a fresh generation. Any pending callback
    // bound to a prior generation is logically cancelled (see
    // `mukei_callback_guard_bump_generation`).
    let generation = mukei_callback_guard_bump_generation(guard_ptr);
    let context_addr = context_ptr as usize;
    let guard_addr = guard_ptr as usize;

    std::thread::spawn(move || {
        let context_ptr_local = context_addr as *mut c_void;
        let guard_ptr_local = guard_addr as *const Inner;
        let payload = match CString::new(input) {
            Ok(payload) => payload,
            Err(_) => return,
        };

        // Single dispatch — `callback_with_guard!` enforces:
        //   1. generation match,
        //   2. `catch_unwind` so a panic does NOT cross the FFI boundary.
        let result: Result<(), GuardError> = callback_with_guard!(guard_ptr_local, generation, {
            callback(context_ptr_local, generation, payload.as_ptr());
            Ok::<(), GuardError>(())
        });

        if let Err(err) = result {
            tracing::warn!(?err, "ffi-shim callback dropped (guard expired or panic)");
        }
    });

    generation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_round_trip_via_canonical_guard() {
        let guard = mukei_acquire_callback_guard();
        let gen_after_1st_bump = mukei_callback_guard_bump_generation(guard);
        assert!(gen_after_1st_bump > 0);
        assert!(mukei_callback_guard_matches(guard, gen_after_1st_bump));

        let gen_after_2nd_bump = mukei_callback_guard_bump_generation(guard);
        assert!(gen_after_2nd_bump > gen_after_1st_bump);
        assert!(!mukei_callback_guard_matches(guard, gen_after_1st_bump));
        assert!(mukei_callback_guard_matches(guard, gen_after_2nd_bump));

        mukei_release_callback_guard(guard);
    }

    extern "C" fn drop_callback(_ctx: *mut c_void, _gen: u64, _tok: *const c_char) {}

    #[test]
    fn null_arguments_are_rejected() {
        let gen = mukei_send_message(
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null(),
            drop_callback,
        );
        assert_eq!(gen, 0);
    }
}
