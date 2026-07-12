//! Android Keystore-backed provider secret storage.
//!
//! The Java helper owns a non-exportable AES/GCM key and persists only
//! ciphertext in the app-private files directory. Rust receives plaintext
//! only for the shortest possible lifetime and returns it in `Zeroizing`.

use zeroize::Zeroizing;

#[cfg(target_os = "android")]
const CLASS: &str = "com.mukei.security.MukeiSecretStore";

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
fn secret_class<'local>(
    env: &mut jni::JNIEnv<'local>,
) -> Result<jni::objects::JClass<'local>, String> {
    use jni::objects::{JClass, JObject, JValue};

    // `FindClass` is unreliable on Rust/Tokio-created native threads because
    // they use the bootstrap class loader. Resolve through the Android app
    // Context's class loader instead.
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
        .map_err(|error| format!("secret class-name allocation failed: {error}"))?;
    let class_name_object = JObject::from(class_name);
    let class_object = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name_object)],
        )
        .and_then(|value| value.l())
        .map_err(|error| format!("load MukeiSecretStore class failed: {error}"))?;
    Ok(JClass::from(class_object))
}


#[cfg(target_os = "android")]
pub fn exists(alias: &str) -> Result<bool, String> {
    use jni::objects::{JObject, JString};

    if alias.is_empty()
        || alias.len() > 64
        || !alias
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        return Err("invalid secret alias".to_string());
    }

    with_env(|env| {
        let context = ndk_context::android_context();
        let context_object =
            unsafe { JObject::from_raw(context.context().cast::<jni::sys::_jobject>()) };
        let files_dir = env
            .call_method(&context_object, "getFilesDir", "()Ljava/io/File;", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("resolve app-private files directory failed: {error}"))?;
        let absolute_path = env
            .call_method(&files_dir, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("resolve app-private files path failed: {error}"))?;
        let absolute_path = JString::from(absolute_path);
        let base: String = env
            .get_string(&absolute_path)
            .map_err(|error| format!("convert app-private files path failed: {error}"))?
            .into();
        Ok(std::path::Path::new(&base)
            .join("secrets")
            .join(format!("{alias}.enc"))
            .is_file())
    })
}

#[cfg(target_os = "android")]
pub fn store(alias: &str, secret: &[u8]) -> Result<(), String> {
    use jni::objects::{JObject, JValue};
    with_env(|env| {
        let class = secret_class(env)?;
        let alias = env
            .new_string(alias)
            .map_err(|error| format!("secret alias allocation failed: {error}"))?;
        let secret = env
            .byte_array_from_slice(secret)
            .map_err(|error| format!("secret byte-array allocation failed: {error}"))?;
        let alias_obj = JObject::from(alias);
        let secret_obj = JObject::from(secret);
        let stored = env
            .call_static_method(
                &class,
                "store",
                "(Ljava/lang/String;[B)Z",
                &[JValue::Object(&alias_obj), JValue::Object(&secret_obj)],
            )
            .and_then(|value| value.z())
            .map_err(|error| format!("Android Keystore store failed: {error}"))?;
        if stored {
            Ok(())
        } else {
            Err("Android Keystore refused secret write".to_string())
        }
    })
}

#[cfg(target_os = "android")]
pub fn load(alias: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    use jni::objects::{JByteArray, JObject, JValue};
    with_env(|env| {
        let class = secret_class(env)?;
        let alias = env
            .new_string(alias)
            .map_err(|error| format!("secret alias allocation failed: {error}"))?;
        let alias_obj = JObject::from(alias);
        let object = env
            .call_static_method(
                &class,
                "load",
                "(Ljava/lang/String;)[B",
                &[JValue::Object(&alias_obj)],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Android Keystore load failed: {error}"))?;
        if object.is_null() {
            return Ok(None);
        }
        let array = JByteArray::from(object);
        let bytes = env
            .convert_byte_array(&array)
            .map_err(|error| format!("secret byte-array conversion failed: {error}"))?;
        Ok(Some(Zeroizing::new(bytes)))
    })
}

#[cfg(target_os = "android")]
pub fn delete(alias: &str) -> Result<(), String> {
    use jni::objects::{JObject, JValue};
    with_env(|env| {
        let class = secret_class(env)?;
        let alias = env
            .new_string(alias)
            .map_err(|error| format!("secret alias allocation failed: {error}"))?;
        let alias_obj = JObject::from(alias);
        let deleted = env
            .call_static_method(
                &class,
                "delete",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&alias_obj)],
            )
            .and_then(|value| value.z())
            .map_err(|error| format!("Android Keystore delete failed: {error}"))?;
        if deleted {
            Ok(())
        } else {
            Err("Android Keystore refused secret deletion".to_string())
        }
    })
}

#[cfg(not(target_os = "android"))]
pub fn store(_alias: &str, _secret: &[u8]) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "android"))]
pub fn load(_alias: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    Ok(None)
}

#[cfg(not(target_os = "android"))]
pub fn delete(_alias: &str) -> Result<(), String> {
    Ok(())
}
