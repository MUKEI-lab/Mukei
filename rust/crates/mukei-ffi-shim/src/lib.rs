//! Manual C-FFI escape hatch — TRD §1.3.2.
//!
//! This crate is the **only** place in the workspace that exposes a stable
//! `extern "C"` surface to Qt/QML for the manual fallback path. Every
//! callback that crosses this boundary is paired with a
//! [`mukei_core::guard::CallbackGuard`] and dispatched through the
//! [`mukei_core::callback_with_guard!`] macro so that:
//!
//!  1. A destroyed or ABA-reused QObject can never receive a
//!     tail-callback (generation or instance mismatch → drop).
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
///
/// # Safety
///
/// `guard_ptr` must have been returned by [`mukei_acquire_callback_guard`]
/// and must be released exactly once.
#[no_mangle]
pub unsafe extern "C" fn mukei_release_callback_guard(guard_ptr: *const Inner) {
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
///
/// # Safety
///
/// `guard_ptr` must be a live pointer returned by
/// [`mukei_acquire_callback_guard`] and not yet released.
#[no_mangle]
pub unsafe extern "C" fn mukei_callback_guard_current_generation(guard_ptr: *const Inner) -> u64 {
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
///
/// # Safety
///
/// `guard_ptr` must be a live pointer returned by
/// [`mukei_acquire_callback_guard`] and not yet released.
#[no_mangle]
pub unsafe extern "C" fn mukei_callback_guard_bump_generation(guard_ptr: *const Inner) -> u64 {
    if guard_ptr.is_null() {
        return 0;
    }
    // SAFETY: see above.
    let inner = unsafe { &*guard_ptr };
    inner.generation.fetch_add(1, Ordering::AcqRel) + 1
}

/// True iff `generation` matches the guard's current generation.
///
/// # Safety
///
/// `guard_ptr` must be a live pointer returned by
/// [`mukei_acquire_callback_guard`] and not yet released.
#[no_mangle]
pub unsafe extern "C" fn mukei_callback_guard_matches(
    guard_ptr: *const Inner,
    generation: u64,
) -> bool {
    if guard_ptr.is_null() {
        return false;
    }
    // SAFETY: see above.
    let inner = unsafe { &*guard_ptr };
    inner.generation.load(Ordering::Acquire) == generation
}

/// True iff `(generation, instance_id)` matches the live guard. This is
/// the full ABA-safe predicate; `mukei_callback_guard_matches` remains
/// for legacy callers that only understand generation snapshots.
///
/// # Safety
///
/// `guard_ptr` must be a live pointer returned by
/// [`mukei_acquire_callback_guard`] and not yet released.
#[no_mangle]
pub unsafe extern "C" fn mukei_callback_guard_matches_instance(
    guard_ptr: *const Inner,
    generation: u64,
    instance_id: u64,
) -> bool {
    if guard_ptr.is_null() {
        return false;
    }
    // SAFETY: see `mukei_callback_guard_matches`.
    let inner = unsafe { &*guard_ptr };
    inner.generation.load(Ordering::Acquire) == generation && inner.instance_id() == instance_id
}

/// Bump the guard's generation as the "stop the world" signal. Any
/// callback still scheduled against the previous generation will be
/// dropped on its next dispatch.
///
/// # Safety
///
/// `guard_ptr` must be a live pointer returned by
/// [`mukei_acquire_callback_guard`] and not yet released.
#[no_mangle]
pub unsafe extern "C" fn mukei_stop_generation(guard_ptr: *const Inner) {
    // SAFETY: this function has the same live-guard precondition.
    let _ = unsafe { mukei_callback_guard_bump_generation(guard_ptr) };
}

/// Read the process-unique `instance_id` assigned at guard
/// construction. Returns 0 if `guard_ptr` is NULL.
///
/// Architect review GH #53 — the instance_id defeats the heap-allocator
/// reuse window: even if the same address is recycled by a later
/// acquire, the new Inner carries a different instance_id so a caller
/// who captured the old (pointer, generation, instance_id) triple will
/// detect the reuse and drop the stale callback.
///
/// # Safety
///
/// `guard_ptr` must be a live pointer returned by
/// [`mukei_acquire_callback_guard`] and not yet released.
#[no_mangle]
pub unsafe extern "C" fn mukei_callback_guard_instance_id(guard_ptr: *const Inner) -> u64 {
    if guard_ptr.is_null() {
        return 0;
    }
    // SAFETY: pointer was produced by `mukei_acquire_callback_guard` and
    // has not been released yet (caller's contract).
    let inner = unsafe { &*guard_ptr };
    inner.instance_id()
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

/// # Safety
///
/// `user_input` must point to a valid NUL-terminated UTF-8 string for the
/// duration of the call. `context_ptr` and `guard_ptr` must both be live
/// pointers paired with the provided callback contract; `guard_ptr` must come
/// from [`mukei_acquire_callback_guard`] and remain valid until release.
#[no_mangle]
pub unsafe extern "C" fn mukei_send_message(
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
    let generation = unsafe { mukei_callback_guard_bump_generation(guard_ptr) };
    let instance_id = unsafe { mukei_callback_guard_instance_id(guard_ptr) };
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
        let result: Result<(), GuardError> =
            callback_with_guard!(guard_ptr_local, generation, instance_id, {
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
        let gen_after_1st_bump = unsafe { mukei_callback_guard_bump_generation(guard) };
        assert!(gen_after_1st_bump > 0);
        assert!(unsafe { mukei_callback_guard_matches(guard, gen_after_1st_bump) });

        let gen_after_2nd_bump = unsafe { mukei_callback_guard_bump_generation(guard) };
        assert!(gen_after_2nd_bump > gen_after_1st_bump);
        assert!(!unsafe { mukei_callback_guard_matches(guard, gen_after_1st_bump) });
        assert!(unsafe { mukei_callback_guard_matches(guard, gen_after_2nd_bump) });

        unsafe { mukei_release_callback_guard(guard) };
    }

    extern "C" fn drop_callback(_ctx: *mut c_void, _gen: u64, _tok: *const c_char) {}

    #[test]
    fn null_arguments_are_rejected() {
        let gen = unsafe {
            mukei_send_message(
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                drop_callback,
            )
        };
        assert_eq!(gen, 0);
    }

    /// Architect review GH #49 / #50 — C-header drift detector.
    ///
    /// The hand-maintained `include/mukei_ffi_shim.h` is the source of
    /// truth for the C ABI shipped to Qt/QML. This test parses the
    /// header and asserts that every Rust-side `#[no_mangle] pub extern
    /// "C" fn ...` symbol is declared in the header, and vice versa.
    ///
    /// Why hand-maintained vs `cbindgen`: see the rationale block at
    /// the top of `mukei_ffi_shim.h`. The short version is that a
    /// build-time codegen tool would (a) introduce a host-build
    /// dependency, (b) defeat reproducible-build invariants, and
    /// (c) widen the supply-chain attack surface.
    #[test]
    fn c_header_lists_every_exported_symbol() {
        let mut header_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        header_path.push("include");
        header_path.push("mukei_ffi_shim.h");
        let header = std::fs::read_to_string(&header_path)
            .unwrap_or_else(|e| panic!("missing C header at {header_path:?}: {e}"));

        // Canonical list of every `#[no_mangle] pub extern "C" fn` in
        // this file. Adding a new export REQUIRES adding the symbol
        // both here and in the C header. Removing one REQUIRES
        // removing it from both. The Rust-side compiler enforces the
        // function definitions; this test enforces the header.
        let exported: &[&str] = &[
            "mukei_acquire_callback_guard",
            "mukei_release_callback_guard",
            "mukei_callback_guard_current_generation",
            "mukei_callback_guard_bump_generation",
            "mukei_callback_guard_matches",
            "mukei_callback_guard_matches_instance",
            "mukei_stop_generation",
            "mukei_callback_guard_instance_id",
            "mukei_initialize",
            "mukei_send_message",
        ];
        for sym in exported {
            assert!(
                header.contains(sym),
                "C header `mukei_ffi_shim.h` is missing the declaration for `{sym}`. \
                 Add it (matching the Rust signature) and re-run the test."
            );
        }

        // Defence-in-depth: catch the inverse drift where the header
        // gains a phantom symbol the Rust source no longer exports.
        // We grep for every `mukei_<identifier>(` occurrence in the
        // header (including the case where the function name is glued
        // to its open-paren, e.g. `mukei_acquire_callback_guard(void)`)
        // and ensure each one is in the canonical list above.
        for line in header.lines() {
            let trimmed = line.trim_start();
            // Skip comment lines so prose mentions of "mukei_*(...)" in
            // the rationale block don't trip the detector.
            if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
                continue;
            }
            // Walk the line manually so we treat the `(` as a strict
            // identifier terminator (not just a whitespace boundary).
            let chars: Vec<char> = line.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == 'm' && line[i..].starts_with("mukei_") {
                    let start = i;
                    while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                        i += 1;
                    }
                    let sym: String = chars[start..i].iter().collect();
                    if i < chars.len() && chars[i] == '(' && !exported.contains(&sym.as_str()) {
                        panic!(
                            "C header declares `{sym}(...)` but Rust does not export it. \
                             Either remove it from `mukei_ffi_shim.h` or add the \
                             matching `#[no_mangle] pub extern \"C\"` to `lib.rs`."
                        );
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
}
