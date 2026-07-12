//! Android Storage Access Framework permission boundary.
//!
//! The Java helper owns `ContentResolver` interactions. Rust receives only
//! a classified permission result and bounded metadata; no filesystem path
//! is derived from a `content://` URI.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    Failed,
    Transient,
    #[cfg_attr(
        not(target_os = "android"),
        expect(
            dead_code,
            reason = "constructed only by the Android SAF persistence result"
        )
    )]
    Persisted,
    NotRequired,
}

impl PermissionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Transient => "transient",
            Self::Persisted => "persisted",
            Self::NotRequired => "not_required",
        }
    }
}

#[cfg(target_os = "android")]
const CLASS: &str = "com.mukei.storage.MukeiDocumentAccess";

#[cfg(target_os = "android")]
fn with_env<T>(f: impl FnOnce(&mut jni::JNIEnv<'_>) -> Result<T, String>) -> Result<T, String> {
    let context = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast()) }
        .map_err(|error| format!("attach Android JavaVM failed: {error}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| format!("attach JNI thread failed: {error}"))?;
    f(&mut env)
}

#[cfg(target_os = "android")]
fn document_class<'local>(
    env: &mut jni::JNIEnv<'local>,
) -> Result<jni::objects::JClass<'local>, String> {
    use jni::objects::{JClass, JObject, JValue};
    let context = ndk_context::android_context();
    let context_object =
        unsafe { JObject::from_raw(context.context().cast::<jni::sys::_jobject>()) };
    let loader = env
        .call_method(
            &context_object,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .and_then(|value| value.l())
        .map_err(|error| format!("resolve Android app class loader failed: {error}"))?;
    let class_name = env
        .new_string(CLASS)
        .map_err(|error| format!("document class-name allocation failed: {error}"))?;
    let class_name_object = JObject::from(class_name);
    let class_object = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name_object)],
        )
        .and_then(|value| value.l())
        .map_err(|error| format!("load MukeiDocumentAccess class failed: {error}"))?;
    Ok(JClass::from(class_object))
}

#[cfg(target_os = "android")]
fn call_int(method: &str, target: &str) -> Result<i32, String> {
    use jni::objects::{JObject, JValue};
    with_env(|env| {
        let class = document_class(env)?;
        let target = env.new_string(target).map_err(|error| error.to_string())?;
        let target = JObject::from(target);
        env.call_static_method(
            &class,
            method,
            "(Ljava/lang/String;)I",
            &[JValue::Object(&target)],
        )
        .and_then(|value| value.i())
        .map_err(|error| format!("Android document {method} failed: {error}"))
    })
}

#[cfg(target_os = "android")]
fn call_bool(method: &str, target: &str) -> Result<bool, String> {
    use jni::objects::{JObject, JValue};
    with_env(|env| {
        let class = document_class(env)?;
        let target = env.new_string(target).map_err(|error| error.to_string())?;
        let target = JObject::from(target);
        env.call_static_method(
            &class,
            method,
            "(Ljava/lang/String;)Z",
            &[JValue::Object(&target)],
        )
        .and_then(|value| value.z())
        .map_err(|error| format!("Android document {method} failed: {error}"))
    })
}

#[cfg(target_os = "android")]
pub fn persist_read_permission(target: &str) -> Result<PermissionState, String> {
    Ok(match call_int("persistReadPermission", target)? {
        1 => PermissionState::Persisted,
        0 => PermissionState::Transient,
        2 => PermissionState::NotRequired,
        _ => PermissionState::Failed,
    })
}

#[cfg(not(target_os = "android"))]
pub fn persist_read_permission(target: &str) -> Result<PermissionState, String> {
    if target.starts_with("file://") {
        Ok(PermissionState::NotRequired)
    } else {
        Ok(PermissionState::Transient)
    }
}

#[cfg(target_os = "android")]
pub fn release_read_permission(target: &str) -> Result<(), String> {
    if call_bool("releaseReadPermission", target)? {
        Ok(())
    } else {
        Err("Android refused document permission release".to_string())
    }
}

#[cfg(not(target_os = "android"))]
pub fn release_read_permission(_target: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "android")]
pub fn can_read(target: &str) -> Result<bool, String> {
    call_bool("canRead", target)
}

#[cfg(not(target_os = "android"))]
pub fn can_read(target: &str) -> Result<bool, String> {
    let Some(path) = target.strip_prefix("file://") else {
        return Ok(false);
    };
    Ok(std::path::Path::new(path).is_file())
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;

    #[test]
    fn host_file_uri_is_readable_without_android_permission() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private.txt");
        std::fs::write(&path, b"private content").unwrap();
        let uri = format!("file://{}", path.display());
        assert_eq!(
            persist_read_permission(&uri).unwrap(),
            PermissionState::NotRequired
        );
        assert!(can_read(&uri).unwrap());
        release_read_permission(&uri).unwrap();
    }

    #[test]
    fn host_content_uri_is_never_claimed_as_readable() {
        let uri = "content://provider/document/42";
        assert_eq!(
            persist_read_permission(uri).unwrap(),
            PermissionState::Transient
        );
        assert!(!can_read(uri).unwrap());
    }
}
