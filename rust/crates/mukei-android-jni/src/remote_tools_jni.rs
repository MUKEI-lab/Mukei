use jni::objects::{JByteArray, JObject};
use jni::sys::{jbyteArray, jlong};
use jni::JNIEnv;
use zeroize::{Zeroize, Zeroizing};

const MAX_PROVIDER_KEY_BYTES: usize = 16 * 1024;

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_configureRemoteTools(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    brave_key: JByteArray<'_>,
    tavily_key: JByteArray<'_>,
) -> jbyteArray {
    let response = crate::guarded_bytes(|| {
        let Some(runtime) = crate::runtime_entry(handle) else {
            return crate::invalid_handle_payload();
        };
        let mut brave_bytes = match env.convert_byte_array(&brave_key) {
            Ok(value) => value,
            Err(_) => return crate::error_payload(
                "invalid_remote_credentials",
                "Unreadable Brave credential.",
            ),
        };
        let mut tavily_bytes = match env.convert_byte_array(&tavily_key) {
            Ok(value) => value,
            Err(_) => {
                brave_bytes.zeroize();
                return crate::error_payload(
                    "invalid_remote_credentials",
                    "Unreadable Tavily credential.",
                );
            }
        };
        if brave_bytes.is_empty()
            || brave_bytes.len() > MAX_PROVIDER_KEY_BYTES
            || tavily_bytes.is_empty()
            || tavily_bytes.len() > MAX_PROVIDER_KEY_BYTES
        {
            brave_bytes.zeroize();
            tavily_bytes.zeroize();
            return crate::error_payload(
                "invalid_remote_credentials",
                "Provider credential sizes are outside supported bounds.",
            );
        }
        let brave = match String::from_utf8(std::mem::take(&mut brave_bytes)) {
            Ok(value) => Zeroizing::new(value),
            Err(error) => {
                let mut bytes = error.into_bytes();
                bytes.zeroize();
                tavily_bytes.zeroize();
                return crate::error_payload(
                    "invalid_remote_credentials",
                    "Brave credential is not UTF-8.",
                );
            }
        };
        let tavily = match String::from_utf8(std::mem::take(&mut tavily_bytes)) {
            Ok(value) => Zeroizing::new(value),
            Err(error) => {
                let mut bytes = error.into_bytes();
                bytes.zeroize();
                return crate::error_payload(
                    "invalid_remote_credentials",
                    "Tavily credential is not UTF-8.",
                );
            }
        };
        match runtime.configure_remote_tools(brave, tavily) {
            Ok(()) => crate::serialize(&serde_json::json!({"accepted": true})),
            Err(error) => {
                crate::error_payload("remote_credentials_rejected", error.error_code())
            }
        }
    });
    crate::to_java_bytes(&mut env, &response)
}
