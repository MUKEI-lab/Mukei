//! Manual C-FFI escape hatch — TRD §1.3.2.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

#[repr(C)]
pub struct CallbackGuard {
    generation: u64,
}

pub type TokenCallback = extern "C" fn(context_ptr: *mut c_void, generation: u64, token: *const c_char);

#[inline]
unsafe fn generation_atomic<'a>(guard: *mut CallbackGuard) -> &'a AtomicU64 {
    AtomicU64::from_ptr(ptr::addr_of_mut!((*guard).generation))
}

#[no_mangle]
pub extern "C" fn mukei_acquire_callback_guard() -> *mut CallbackGuard {
    Box::into_raw(Box::new(CallbackGuard { generation: 0 }))
}

#[no_mangle]
pub extern "C" fn mukei_release_callback_guard(guard: *mut CallbackGuard) {
    if guard.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(guard));
    }
}

#[no_mangle]
pub extern "C" fn mukei_callback_guard_current_generation(guard: *mut CallbackGuard) -> u64 {
    if guard.is_null() {
        return 0;
    }
    unsafe { generation_atomic(guard).load(Ordering::SeqCst) }
}

#[no_mangle]
pub extern "C" fn mukei_callback_guard_bump_generation(guard: *mut CallbackGuard) -> u64 {
    if guard.is_null() {
        return 0;
    }
    unsafe { generation_atomic(guard).fetch_add(1, Ordering::SeqCst) + 1 }
}

#[no_mangle]
pub extern "C" fn mukei_callback_guard_matches(guard: *mut CallbackGuard, generation: u64) -> bool {
    if guard.is_null() {
        return false;
    }
    unsafe { generation_atomic(guard).load(Ordering::SeqCst) == generation }
}

#[no_mangle]
pub extern "C" fn mukei_stop_generation(guard: *mut CallbackGuard) {
    let _ = mukei_callback_guard_bump_generation(guard);
}

#[no_mangle]
pub extern "C" fn mukei_initialize(_config_path: *const c_char) -> bool {
    true
}

#[no_mangle]
pub extern "C" fn mukei_send_message(
    user_input: *const c_char,
    context_ptr: *mut c_void,
    guard: *mut CallbackGuard,
    callback: TokenCallback,
) -> u64 {
    if user_input.is_null() || context_ptr.is_null() || guard.is_null() {
        return 0;
    }

    let input = match unsafe { CStr::from_ptr(user_input) }.to_str() {
        Ok(value) => value.to_owned(),
        Err(_) => return 0,
    };

    let generation = mukei_callback_guard_bump_generation(guard);
    let context_addr = context_ptr as usize;
    let guard_addr = guard as usize;

    std::thread::spawn(move || {
        let context_ptr = context_addr as *mut c_void;
        let guard_ptr = guard_addr as *mut CallbackGuard;
        let payload = match CString::new(input) {
            Ok(payload) => payload,
            Err(_) => return,
        };
        let live = unsafe { generation_atomic(guard_ptr).load(Ordering::SeqCst) };
        if live != generation {
            return;
        }
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            callback(context_ptr, generation, payload.as_ptr());
        }));
    });

    generation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_helpers_round_trip() {
        let guard = mukei_acquire_callback_guard();
        let gen1 = mukei_callback_guard_bump_generation(guard);
        assert!(mukei_callback_guard_matches(guard, gen1));
        let gen2 = mukei_callback_guard_bump_generation(guard);
        assert!(gen2 > gen1);
        assert!(!mukei_callback_guard_matches(guard, gen1));
        mukei_release_callback_guard(guard);
    }
}
