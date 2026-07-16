#![allow(non_snake_case)]

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicI64, Ordering};

use jni::objects::{JByteArray, JObject, JString};
use jni::sys::{jbyteArray, jint, jlong};
use jni::JNIEnv;
use mukei_core::application_runtime::RuntimeConfig;
use mukei_core::ui_protocol::{
    validate_command, ClientKind, CommandAcknowledgementV2, CommandEnvelopeV2, EventBatchV2,
    ProtocolCapabilitySnapshot, ProtocolVersion, RejectionReason, RuntimeContractSnapshot,
    SnapshotDomainV2, SnapshotEnvelopeV2, CAP_ANDROID_JNI_TRANSPORT,
    MAX_COMMAND_ENVELOPE_BYTES, MAX_EVENT_BATCH_ITEMS,
};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::json;
use uuid::Uuid;

const MAX_DRAIN_TIMEOUT_MS: jlong = 30_000;

#[derive(Clone)]
struct RuntimeTransportEntry {
    session_id: String,
    config: RuntimeConfig,
}

static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
static RUNTIMES: Lazy<Mutex<HashMap<jlong, RuntimeTransportEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn runtime_entry(handle: jlong) -> Option<RuntimeTransportEntry> {
    if handle <= 0 {
        return None;
    }
    RUNTIMES.lock().get(&handle).cloned()
}

fn panic_payload() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "error": {
            "code": "native_panic_contained",
            "message": "The JNI boundary contained an unexpected panic."
        }
    }))
    .unwrap_or_else(|_| b"{}".to_vec())
}

fn guarded_bytes(operation: impl FnOnce() -> Vec<u8>) -> Vec<u8> {
    catch_unwind(AssertUnwindSafe(operation)).unwrap_or_else(|_| panic_payload())
}

fn to_java_bytes(env: &mut JNIEnv<'_>, bytes: &[u8]) -> jbyteArray {
    env.byte_array_from_slice(bytes)
        .map(|array| array.into_raw())
        .unwrap_or_else(|_| null_mut())
}

fn serialize<T: serde::Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|_| panic_payload())
}

fn invalid_handle_payload() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "error": {
            "code": "invalid_runtime_handle",
            "message": "The native runtime handle is not active."
        }
    }))
    .unwrap_or_else(|_| b"{}".to_vec())
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_createRuntime(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    config_json: JByteArray<'_>,
) -> jlong {
    catch_unwind(AssertUnwindSafe(|| {
        let config_bytes = env.convert_byte_array(&config_json).unwrap_or_default();
        if config_bytes.is_empty() || config_bytes.len() > MAX_COMMAND_ENVELOPE_BYTES {
            return 0;
        }
        let config: RuntimeConfig = match serde_json::from_slice(&config_bytes) {
            Ok(value) => value,
            Err(_) => return 0,
        };

        let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        if handle <= 0 {
            return 0;
        }
        RUNTIMES.lock().insert(
            handle,
            RuntimeTransportEntry {
                session_id: Uuid::new_v4().to_string(),
                config,
            },
        );
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
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_protocolCapabilities(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jbyteArray {
    let response = guarded_bytes(|| {
        let Some(runtime) = runtime_entry(handle) else {
            return invalid_handle_payload();
        };
        let contract = RuntimeContractSnapshot {
            client_kind: ClientKind::Android,
            runtime_session_id: runtime.session_id,
            protocol: ProtocolCapabilitySnapshot::current()
                .with_transport(CAP_ANDROID_JNI_TRANSPORT),
            snapshot_schema_version: 1,
        };
        serialize(&contract)
    });
    to_java_bytes(&mut env, &response)
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_submitCommand(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    command_json: JByteArray<'_>,
) -> jbyteArray {
    let response = guarded_bytes(|| {
        if runtime_entry(handle).is_none() {
            return invalid_handle_payload();
        }
        let command_bytes = match env.convert_byte_array(&command_json) {
            Ok(bytes) => bytes,
            Err(_) => {
                return serialize(&CommandAcknowledgementV2::rejected(
                    None,
                    RejectionReason::InvalidPayload,
                ))
            }
        };
        if command_bytes.is_empty() || command_bytes.len() > MAX_COMMAND_ENVELOPE_BYTES {
            return serialize(&CommandAcknowledgementV2::rejected(
                None,
                RejectionReason::InvalidPayload,
            ));
        }
        let command: CommandEnvelopeV2 = match serde_json::from_slice(&command_bytes) {
            Ok(value) => value,
            Err(_) => {
                return serialize(&CommandAcknowledgementV2::rejected(
                    None,
                    RejectionReason::InvalidPayload,
                ))
            }
        };
        let acknowledgement = match validate_command(command.clone()) {
            Ok(_) => CommandAcknowledgementV2::rejected(
                Some(&command),
                RejectionReason::BackendUnavailable,
            ),
            Err(reason) => CommandAcknowledgementV2::rejected(Some(&command), reason),
        };
        serialize(&acknowledgement)
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
        let Some(runtime) = runtime_entry(handle) else {
            return invalid_handle_payload();
        };
        if !(1..=MAX_EVENT_BATCH_ITEMS as jint).contains(&maximum_events)
            || !(0..=MAX_DRAIN_TIMEOUT_MS).contains(&timeout_milliseconds)
        {
            return serde_json::to_vec(&json!({
                "error": {
                    "code": "invalid_drain_request",
                    "message": "The event batch size or timeout is invalid."
                }
            }))
            .unwrap_or_else(|_| b"{}".to_vec());
        }
        serialize(&EventBatchV2 {
            protocol_version: ProtocolVersion::CURRENT,
            runtime_session_id: runtime.session_id,
            drained_at: chrono::Utc::now(),
            events: Vec::new(),
            has_more: false,
        })
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
        let Some(runtime) = runtime_entry(handle) else {
            return invalid_handle_payload();
        };
        let domain: String = match env.get_string(&domain) {
            Ok(value) => value.into(),
            Err(_) => {
                return serde_json::to_vec(&json!({
                    "error": { "code": "invalid_domain" }
                }))
                .unwrap_or_else(|_| b"{}".to_vec())
            }
        };
        let Some(domain) = SnapshotDomainV2::parse(domain.trim()) else {
            return serde_json::to_vec(&json!({
                "error": { "code": "unsupported_snapshot_domain" }
            }))
            .unwrap_or_else(|_| b"{}".to_vec());
        };
        let payload = match domain {
            SnapshotDomainV2::Application => json!({
                "state": "transport_ready_core_runtime_unbound",
                "app_data_dir": runtime.config.app_data_dir,
            }),
            SnapshotDomainV2::Protocol => json!({
                "capabilities": ProtocolCapabilitySnapshot::current()
                    .with_transport(CAP_ANDROID_JNI_TRANSPORT),
            }),
            SnapshotDomainV2::Settings => json!({ "values": {} }),
            SnapshotDomainV2::Operations => json!({ "active": [], "replay_entries": 0 }),
        };
        serialize(&SnapshotEnvelopeV2 {
            protocol_version: ProtocolVersion::CURRENT,
            runtime_session_id: runtime.session_id,
            domain,
            schema_version: 1,
            generated_at: chrono::Utc::now(),
            payload,
        })
    });
    to_java_bytes(&mut env, &response)
}
