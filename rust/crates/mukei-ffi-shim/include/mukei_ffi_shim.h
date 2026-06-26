/*
 * mukei_ffi_shim.h — canonical C header for the Mukei manual-FFI escape
 * hatch (TRD §1.3.2, architect review GH #49 / #50).
 *
 * SOURCE OF TRUTH
 *   This file is the canonical C-side declaration of every symbol
 *   exported by `crates/mukei-ffi-shim`. It is **hand-maintained** and
 *   committed to the repo for two reasons:
 *
 *     1. **Reproducible Android builds.** The bridge crate's NDK build
 *        must be able to link against this header without running a
 *        `cbindgen` step at every build, which would (a) add a
 *        host-side build dependency, (b) introduce non-determinism, and
 *        (c) defeat the supply-chain audit (each new tool is a new
 *        attack surface, per architect review GH #7).
 *
 *     2. **ABI drift catch.** The companion Rust test
 *        `tests::c_header_lists_every_exported_symbol` in
 *        `crates/mukei-ffi-shim/src/lib.rs` greps THIS file for the
 *        exact set of symbols emitted by `#[no_mangle] pub extern "C"`
 *        in the shim's `lib.rs`. Adding, removing, or renaming any
 *        symbol on the Rust side without updating this header fails
 *        the test before the broken ABI ever ships.
 *
 * SAFETY CONTRACT (cross-references)
 *   - Every `*const MukeiCallbackGuardInner` returned by
 *     `mukei_acquire_callback_guard` MUST be released exactly once via
 *     `mukei_release_callback_guard`. Calling release twice on the same
 *     pointer is undefined behaviour (double-free of an `Arc<Inner>`).
 *   - `MukeiTokenCallback` MAY be called from any Rust runtime thread.
 *     The implementation MUST be re-entrant and MUST NOT itself call
 *     back into `mukei_send_message` on the same thread.
 *   - A panic inside `MukeiTokenCallback` is caught by Rust's
 *     `catch_unwind` (TRD §1.3.2 / GH #45). The C-side observer can
 *     treat the callback as best-effort.
 *
 * REQUIRED BUILD INVARIANT
 *   The workspace `[profile.*]` settings MUST keep `panic = "unwind"`
 *   for every profile that links this shim. `panic = "abort"` makes
 *   the `catch_unwind` inside `callback_with_guard!` a no-op, which
 *   would defeat the no-panic-across-FFI guarantee (PRD G1, TRD §1.3).
 */

#ifndef MUKEI_FFI_SHIM_H
#define MUKEI_FFI_SHIM_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* --------------------------------------------------------------------
 * Opaque types
 * ------------------------------------------------------------------ */

/*
 * Opaque ABI handle for the Rust-side `Arc<Inner>`. The C side never
 * dereferences it; only round-trips it back into the shim.
 */
typedef struct MukeiCallbackGuardInner MukeiCallbackGuardInner;

/* --------------------------------------------------------------------
 * Callback signatures
 * ------------------------------------------------------------------ */

/*
 * Streaming token callback.
 *
 *   context_ptr — opaque pointer the QObject passed at bind time.
 *   generation  — guard generation at the time of the call.
 *   token       — UTF-8, NUL-terminated chunk of streamed text. Borrowed
 *                 for the lifetime of the call; copy if you need to
 *                 outlive the callback.
 *
 * MUST NOT throw / panic / longjmp. Rust catches panics, but C++ side
 * exceptions across this boundary are undefined behaviour.
 */
typedef void (*MukeiTokenCallback)(void* context_ptr,
                                   uint64_t generation,
                                   const char* token);

/* --------------------------------------------------------------------
 * Guard lifecycle (TRD §1.3.2)
 * ------------------------------------------------------------------ */

/*
 * Allocate a fresh callback guard. Returns NULL only on allocation
 * failure (unlikely). The returned pointer MUST be released exactly
 * once via `mukei_release_callback_guard`.
 */
const MukeiCallbackGuardInner* mukei_acquire_callback_guard(void);

/*
 * Release the guard and drop its heap allocation. Idempotent for NULL
 * input; otherwise MUST be called exactly once per `acquire`.
 */
void mukei_release_callback_guard(const MukeiCallbackGuardInner* guard_ptr);

/*
 * Read the current generation counter. Returns 0 if `guard_ptr` is
 * NULL (matches the rejected-by-default contract). Use this when
 * binding a callback so the dispatch loop can detect rebinds.
 */
uint64_t mukei_callback_guard_current_generation(
    const MukeiCallbackGuardInner* guard_ptr);

/*
 * Atomically bump the generation by 1 and return the new value. Used
 * when rebinding the guard to a fresh QObject (Activity rotation,
 * etc.). Returns 0 on NULL input. Saturates at u64::MAX - 1 → u64::MAX
 * (logical tombstone).
 */
uint64_t mukei_callback_guard_bump_generation(
    const MukeiCallbackGuardInner* guard_ptr);

/*
 * Compare the current guard generation to a previously-captured
 * snapshot. Used by the dispatch loop's "still alive" check.
 */
bool mukei_callback_guard_matches(const MukeiCallbackGuardInner* guard_ptr,
                                  uint64_t generation);

/*
 * Permanently invalidate the guard target (tombstone). Any in-flight
 * callback observes a generation mismatch on its next attempt.
 * Distinct from `mukei_release_callback_guard`: the guard heap
 * allocation is NOT freed here; the caller MUST still release it.
 */
void mukei_stop_generation(const MukeiCallbackGuardInner* guard_ptr);

/*
 * Read the process-unique `instance_id` assigned at guard
 * construction. Returns 0 if `guard_ptr` is NULL.
 *
 * Architect review GH #53. Combine with `current_generation` for an
 * ABA-safe bind:
 *
 *   bound_id  = mukei_callback_guard_instance_id(g);
 *   bound_gen = mukei_callback_guard_bump_generation(g);
 *   ...later, before dispatching the callback...
 *   if (mukei_callback_guard_instance_id(g) != bound_id) { drop; }
 *   if (!mukei_callback_guard_matches(g, bound_gen))    { drop; }
 *
 * Even if the underlying heap address is freed and a later `acquire`
 * lands on the same address, the new Inner carries a different
 * instance_id so the stale binding is rejected.
 */
uint64_t mukei_callback_guard_instance_id(
    const MukeiCallbackGuardInner* guard_ptr);

/* --------------------------------------------------------------------
 * Engine entry points
 * ------------------------------------------------------------------ */

/*
 * One-shot initialiser. Returns true on success.
 *
 *   config_path — optional NUL-terminated path to a TOML config file.
 *                 NULL falls back to the embedded defaults.
 *
 * Safe to call multiple times; subsequent calls are no-ops.
 */
bool mukei_initialize(const char* config_path);

/*
 * Submit a user message and stream tokens back through the callback.
 * Returns the generation value the callback will be invoked with so
 * the caller can match deliveries to bind sites.
 *
 *   guard_ptr   — guard allocated via `mukei_acquire_callback_guard`.
 *                 NULL is rejected (returns 0).
 *   context_ptr — opaque pointer relayed to every callback invocation.
 *   input       — UTF-8, NUL-terminated user prompt. NULL is rejected
 *                 (returns 0). Copied internally; lifetime ends with
 *                 this call.
 *   callback    — token-stream sink. MUST NOT be NULL.
 *
 * Returns 0 on NULL-input rejection; otherwise the dispatch generation.
 */
uint64_t mukei_send_message(const MukeiCallbackGuardInner* guard_ptr,
                            void* context_ptr,
                            const char* input,
                            MukeiTokenCallback callback);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* MUKEI_FFI_SHIM_H */
