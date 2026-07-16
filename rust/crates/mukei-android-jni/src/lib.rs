#![allow(non_snake_case)]

use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicI64, Ordering};

use jni::objects::{JByteArray, JObject, JString};
use jni::sys::{jbyteArray, jint, jlong};
use jni::JNIEnv;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::{json, Value};

const MAX_ENVELOPE_BYTES: usize = 64 * 1024;
const MAX_EVENT_BATCH: jint = 256;
const MAX_DRAIN_TIMEOUT_MS: jlong = 30_000;

static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
static RUNTIMES: Lazy<Mutex<HashSet<jlong>>> = Lazy::new(|| Mutex::new(HashSet::new()));

fn runtime_exists(handle: jlong) -> bool {
    handle > 0 && RUNTIMES.lock().contains(&handle)
}

fn json_bytes(value: Value) -> Vec<u8> {
    value.to_string().into_bytes()
}

fn panic_payload() -> Vec<u8> {
    json_bytes(json!({
        "error": {
            "code": "native_panic_contained",
            "message": "The native boundary contained an unexpected panic."
        }
    }))
}

fn guarded_bytes(operation: impl FnOnce() -> Vec<u8>) -> Vec<u8> {
    catch_unwind(AssertUnwindSafe(operation)).unwrap_or_else(|_| panic_payload())
}

fn to_java_bytes(env: &mut JNIEnv<'_>, bytes: &[u8]) -> jbyteArray {
    env.byte_array_from_slice(bytes)
        .map(|array| array.into_raw())
        .unwrap_or_else(|_| null_mut())
}

fn error_payload(code: &str, message: &str) -> Vec<u8> {
    json_bytes(json!({
        "error": {
            "code": code,
            "message": message
        }
    }))
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_createRuntime(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    config_json: JByteArray<'_>,
) -> jlong {
    catch_unwind(AssertUnwindSafe(|| {
        let config = env.convert_byte_array(&config_json).unwrap_or_default();
        if config.len() > MAX_ENVELOPE_BYTES {
            return 0;
        }

        let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        if handle <= 0 {
            return 0;
        }

        RUNTIMES.lock().insert(handle);
        handle
    }))
    .unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_destroyRuntime(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if handle > 0 {
            RUNTIMES.lock().remove(&handle);
        }
    }));
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_submitCommand(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    command_json: JByteArray<'_>,
) -> jbyteArray {
    let response = guarded_bytes(|| {
        if !runtime_exists(handle) {
            return error_payload("invalid_runtime_handle", "The native runtime handle is not active.");
        }

        let command_bytes = match env.convert_byte_array(&command_json) {
            Ok(bytes) => bytes,
            Err(_) => return error_payload("invalid_payload", "The command bytes could not be read."),
        };

        if command_bytes.is_empty() || command_bytes.len() > MAX_ENVELOPE_BYTES {
            return error_payload("invalid_payload", "The command envelope size is invalid.");
        }

        let command: Value = match serde_json::from_slice(&command_bytes) {
            Ok(value) => value,
            Err(_) => return error_payload("invalid_payload", "The command envelope is not valid JSON."),
        };

        let field = |name: &str| {
            command
                .get(name)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };

        json_bytes(json!({
            "protocol_version": { "major": 2, "minor": 0 },
            "accepted": false,
            "command_id": field("command_id"),
            "request_id": field("request_id"),
            "correlation_id": field("correlation_id"),
            "operation_id": command.get("operation_id").cloned().unwrap_or(Value::Null),
            "rejection_reason": "backend_unavailable",
            "detail": "The JNI boundary is active; mukei-core command dispatch is the next integration step."
        }))
    });

    to_java_bytes(&mut env, &response)
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_drainEvents(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    maximum_events: jint,
    timeout_milliseconds: jlong,
) -> jbyteArray {
    let response = guarded_bytes(|| {
        if !runtime_exists(handle) {
            return error_payload("invalid_runtime_handle", "The native runtime handle is not active.");
        }

        if !(1..=MAX_EVENT_BATCH).contains(&maximum_events) {
            return error_payload("invalid_batch_size", "The requested event batch size is invalid.");
        }

        if !(0..=MAX_DRAIN_TIMEOUT_MS).contains(&timeout_milliseconds) {
            return error_payload("invalid_timeout", "The event drain timeout is invalid.");
        }

        b"[]".to_vec()
    });

    to_java_bytes(&mut env, &response)
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_requestSnapshot(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    domain: JString<'_>,
) -> jbyteArray {
    let response = guarded_bytes(|| {
        if !runtime_exists(handle) {
            return error_payload("invalid_runtime_handle", "The native runtime handle is not active.");
        }

        let domain: String = match env.get_string(&domain) {
            Ok(value) => value.into(),
            Err(_) => return error_payload("invalid_domain", "The snapshot domain could not be read."),
        };

        if domain.trim().is_empty() {
            return error_payload("invalid_domain", "The snapshot domain must not be blank.");
        }

        json_bytes(json!({
            "protocol_version": { "major": 2, "minor": 0 },
            "domain": domain,
            "status": "scaffold",
            "payload": {}
        }))
    });

    to_java_bytes(&mut env, &response)
}
