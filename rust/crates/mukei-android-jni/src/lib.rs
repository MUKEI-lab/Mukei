#![allow(non_snake_case)]

mod runtime_registry;
#[cfg(feature = "native_inference")]
mod native_inference;
#[cfg(feature = "rag_runtime")]
mod native_rag;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::null_mut;
use std::sync::Arc;
use std::time::Duration;

use jni::objects::{JByteArray, JObject, JString};
use jni::sys::{jbyteArray, jint, jlong};
use jni::JNIEnv;
use mukei_core::application_runtime::{
    MukeiRuntime, RuntimeConfig, RuntimeServices, RuntimeSnapshotDomain,
};
use mukei_core::platform::{PlatformResponse, MAX_PLATFORM_DRAIN_ITEMS};
use mukei_core::ui_protocol::{
    ClientKind, CommandAcknowledgementV2, CommandEnvelopeV2, EventBatchV2,
    ProtocolVersion, RejectionReason, RuntimeContractSnapshot, SnapshotDomainV2,
    SnapshotEnvelopeV2, CAP_ANDROID_JNI_TRANSPORT, MAX_COMMAND_ENVELOPE_BYTES,
    MAX_EVENT_BATCH_ITEMS,
};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::json;

use runtime_registry::RuntimeRegistry;

const MAX_DRAIN_TIMEOUT_MS: jlong = 30_000;
const MAX_PLATFORM_RESPONSE_BYTES: usize = 512 * 1024;

static RUNTIMES: Lazy<Mutex<RuntimeRegistry>> =
    Lazy::new(|| Mutex::new(RuntimeRegistry::default()));

fn runtime_entry(handle: jlong) -> Option<Arc<MukeiRuntime>> {
    RUNTIMES.lock().get(handle)
}

#[cfg(feature = "native_inference")]
fn runtime_services(config: &RuntimeConfig) -> RuntimeServices {
    use std::path::Path;

    if !native_inference::implementation_available() {
        return RuntimeServices::default();
    }
    let product = mukei_core::config::MukeiConfig::default_for_data_root(Path::new(
        &config.app_data_dir,
    ));
    let max_new_tokens = product
        .watchdog
        .max_token_budget
        .clamp(1, u64::from(u32::MAX)) as u32;
    RuntimeServices {
        backend_factory: Some(Arc::new(
            native_inference::AndroidLlamaBackendFactory::new(
                product.n_ctx,
                product.n_threads,
                product.gpu_layers,
                max_new_tokens,
            ),
        )),
    }
}

#[cfg(not(feature = "native_inference"))]
fn runtime_services(_config: &RuntimeConfig) -> RuntimeServices {
    RuntimeServices::default()
}

fn panic_payload() -> Vec<u8> {
    error_payload(
        "native_panic_contained",
        "The JNI boundary contained an unexpected panic.",
    )
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

fn error_payload(code: &str, message: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "error": {
            "code": code,
            "message": message,
        }
    }))
    .unwrap_or_else(|_| b"{}".to_vec())
}

fn invalid_handle_payload() -> Vec<u8> {
    error_payload(
        "invalid_runtime_handle",
        "The native runtime handle is stale, destroyed, or unknown.",
    )
}

fn runtime_snapshot_domain(domain: SnapshotDomainV2) -> RuntimeSnapshotDomain {
    match domain {
        SnapshotDomainV2::Application => RuntimeSnapshotDomain::Application,
        SnapshotDomainV2::Settings => RuntimeSnapshotDomain::Settings,
        SnapshotDomainV2::Protocol => RuntimeSnapshotDomain::Protocol,
        SnapshotDomainV2::Operations => RuntimeSnapshotDomain::Operations,
    }
}

fn snapshot_response(runtime: &MukeiRuntime, domain: SnapshotDomainV2) -> Vec<u8> {
    let snapshot = match runtime.snapshot(runtime_snapshot_domain(domain)) {
        Ok(snapshot) => snapshot,
        Err(error) => return error_payload("snapshot_unavailable", &error.to_string()),
    };
    serialize(&SnapshotEnvelopeV2 {
        protocol_version: ProtocolVersion::CURRENT,
        runtime_session_id: snapshot.runtime_session_id,
        domain,
        schema_version: snapshot.schema_version,
        generated_at: snapshot.generated_at,
        payload: snapshot.payload,
    })
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_createRuntime(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    config_json: JByteArray<'_>,
) -> jlong {
    catch_unwind(AssertUnwindSafe(|| {
        let config_bytes = match env.convert_byte_array(&config_json) {
            Ok(bytes) => bytes,
            Err(_) => return 0,
        };
        if config_bytes.is_empty() || config_bytes.len() > MAX_COMMAND_ENVELOPE_BYTES {
            return 0;
        }
        let config: RuntimeConfig = match serde_json::from_slice(&config_bytes) {
            Ok(value) => value,
            Err(_) => return 0,
        };
        let services = runtime_services(&config);
        let runtime = match MukeiRuntime::create_with_services(config, services) {
            Ok(runtime) => Arc::new(runtime),
            Err(_) => return 0,
        };
        match RUNTIMES.lock().insert(Arc::clone(&runtime)) {
            Some(handle) => handle,
            None => {
                runtime.shutdown();
                0
            }
        }
    }))
    .unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_shutdownRuntime(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jbyteArray {
    let response = guarded_bytes(|| {
        let Some(runtime) = runtime_entry(handle) else {
            return invalid_handle_payload();
        };
        runtime.shutdown();
        snapshot_response(&runtime, SnapshotDomainV2::Application)
    });
    to_java_bytes(&mut env, &response)
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_destroyRuntime(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let runtime = RUNTIMES.lock().remove(handle);
        if let Some(runtime) = runtime {
            runtime.shutdown();
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
        serialize(&RuntimeContractSnapshot {
            client_kind: ClientKind::Android,
            runtime_session_id: runtime.session_id().to_owned(),
            protocol: runtime
                .capabilities()
                .with_transport(CAP_ANDROID_JNI_TRANSPORT),
            snapshot_schema_version: 2,
        })
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
        let Some(runtime) = runtime_entry(handle) else {
            return invalid_handle_payload();
        };
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
        serialize(&runtime.submit(command))
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
            return error_payload(
                "invalid_drain_request",
                "The event batch size or timeout is outside the supported bounds.",
            );
        }
        let drain = runtime.drain_events(
            maximum_events as usize,
            Duration::from_millis(timeout_milliseconds as u64),
        );
        serialize(&EventBatchV2 {
            protocol_version: ProtocolVersion::CURRENT,
            runtime_session_id: runtime.session_id().to_owned(),
            drained_at: chrono::Utc::now(),
            events: drain.events,
            has_more: drain.has_more,
        })
    });
    to_java_bytes(&mut env, &response)
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_drainPlatformRequests(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    maximum_requests: jint,
    timeout_milliseconds: jlong,
) -> jbyteArray {
    let response = guarded_bytes(|| {
        let Some(runtime) = runtime_entry(handle) else {
            return invalid_handle_payload();
        };
        if !(1..=MAX_PLATFORM_DRAIN_ITEMS as jint).contains(&maximum_requests)
            || !(0..=MAX_DRAIN_TIMEOUT_MS).contains(&timeout_milliseconds)
        {
            return error_payload(
                "invalid_platform_drain_request",
                "The platform batch size or timeout is outside the supported bounds.",
            );
        }
        serialize(&runtime.drain_platform_requests(
            maximum_requests as usize,
            Duration::from_millis(timeout_milliseconds as u64),
        ))
    });
    to_java_bytes(&mut env, &response)
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_submitPlatformResponse(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    response_json: JByteArray<'_>,
) -> jbyteArray {
    let response = guarded_bytes(|| {
        let Some(runtime) = runtime_entry(handle) else {
            return invalid_handle_payload();
        };
        let response_bytes = match env.convert_byte_array(&response_json) {
            Ok(bytes) => bytes,
            Err(_) => return error_payload("invalid_platform_response", "Unreadable response bytes."),
        };
        if response_bytes.is_empty() || response_bytes.len() > MAX_PLATFORM_RESPONSE_BYTES {
            return error_payload(
                "invalid_platform_response",
                "The platform response size is outside the supported bounds.",
            );
        }
        let response: PlatformResponse = match serde_json::from_slice(&response_bytes) {
            Ok(value) => value,
            Err(_) => {
                return error_payload(
                    "invalid_platform_response",
                    "The platform response is not valid Protocol JSON.",
                )
            }
        };
        match runtime.submit_platform_response(response) {
            Ok(()) => serialize(&json!({"accepted": true})),
            Err(error) => error_payload("platform_response_rejected", &error.to_string()),
        }
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
            Err(_) => return error_payload("invalid_domain", "The domain string is unreadable."),
        };
        let Some(domain) = SnapshotDomainV2::parse(domain.trim()) else {
            return error_payload(
                "unsupported_snapshot_domain",
                "The requested snapshot domain is not supported.",
            );
        };
        snapshot_response(&runtime, domain)
    });
    to_java_bytes(&mut env, &response)
}
