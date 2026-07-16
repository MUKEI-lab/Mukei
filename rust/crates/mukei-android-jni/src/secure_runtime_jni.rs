#[cfg(feature = "secure_runtime")]
mod secure_runtime {
    use std::collections::HashMap;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::path::{Component, Path};
    use std::sync::Arc;

    use jni::objects::{JByteArray, JObject};
    use jni::sys::{jbyteArray, jlong};
    use jni::JNIEnv;
    use mukei_core::application_runtime::{MukeiRuntime, RuntimeConfig};
    use mukei_core::diagnostics::{
        install_panic_hook, CrashFingerprint, CrashSink, PanicSink,
    };
    use mukei_core::storage::{DatabaseEncryptionStatus, DatabasePool, Migrator};
    use once_cell::sync::Lazy;
    use parking_lot::Mutex;
    use serde_json::json;
    use zeroize::{Zeroize, Zeroizing};

    #[derive(Debug)]
    struct AndroidPanicSink;

    impl PanicSink for AndroidPanicSink {
        fn on_panic(&self, fingerprint: &CrashFingerprint, _reason: &str) {
            tracing::error!(
                target: "mukei::android::panic",
                fingerprint = %fingerprint,
                "native panic contained and persisted locally"
            );
        }
    }

    struct SecureResources {
        _database: Arc<DatabasePool>,
        encryption_status: DatabaseEncryptionStatus,
    }

    static SECURE_RESOURCES: Lazy<Mutex<HashMap<jlong, SecureResources>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));

    fn app_private_root(app_data_dir: &str) -> Result<&Path, ()> {
        let root = Path::new(app_data_dir);
        if !root.is_absolute()
            || root
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(());
        }
        Ok(root)
    }

    fn install_security_boundary(app_data_root: &Path) -> Result<(), ()> {
        mukei_core::diagnostics::initialize_tracing();
        let crash_sink = CrashSink::open(app_data_root.join("crashes")).map_err(|_| ())?;
        let _ = mukei_core::diagnostics::logger::install_crash_sink(Arc::new(crash_sink));
        install_panic_hook(Arc::new(AndroidPanicSink));
        Ok(())
    }

    fn status_tag(status: DatabaseEncryptionStatus) -> &'static str {
        match status {
            DatabaseEncryptionStatus::Encrypted => "encrypted",
            DatabaseEncryptionStatus::Unavailable => "unavailable",
            DatabaseEncryptionStatus::InvalidKey => "invalid_key",
            DatabaseEncryptionStatus::Corrupted => "corrupted",
            DatabaseEncryptionStatus::MigrationRequired => "migration_required",
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_createSecureRuntime(
        mut env: JNIEnv<'_>,
        _this: JObject<'_>,
        config_json: JByteArray<'_>,
        database_key: JByteArray<'_>,
    ) -> jlong {
        catch_unwind(AssertUnwindSafe(|| {
            let config_bytes = match env.convert_byte_array(&config_json) {
                Ok(bytes) => bytes,
                Err(_) => return 0,
            };
            if config_bytes.is_empty()
                || config_bytes.len() > mukei_core::ui_protocol::MAX_COMMAND_ENVELOPE_BYTES
            {
                return 0;
            }
            let config: RuntimeConfig = match serde_json::from_slice(&config_bytes) {
                Ok(value) => value,
                Err(_) => return 0,
            };
            let app_data_root = match app_private_root(&config.app_data_dir) {
                Ok(root) => root,
                Err(()) => return 0,
            };
            if install_security_boundary(app_data_root).is_err() {
                return 0;
            }

            let mut key_bytes = match env.convert_byte_array(&database_key) {
                Ok(bytes) => bytes,
                Err(_) => return 0,
            };
            if key_bytes.len() != 32 {
                key_bytes.zeroize();
                return 0;
            }

            let product = mukei_core::config::MukeiConfig::default_for_data_root(app_data_root);
            let config_path = app_data_root.join("mukei.toml");
            if product.validate_android_storage_paths(&config_path).is_err() {
                key_bytes.zeroize();
                return 0;
            }
            if product.ensure_storage_directories().is_err() {
                key_bytes.zeroize();
                return 0;
            }
            let database = match DatabasePool::open_with_cipher_key_result(
                &product.database_path,
                Zeroizing::new(key_bytes),
            ) {
                Ok(result) => result,
                Err(error) => {
                    mukei_core::diagnostics::logger::log_error(&error);
                    return 0;
                }
            };

            let encryption_status = database.encryption_status;
            let database_pool = Arc::new(database.pool);
            let migrator = Migrator::embedded();
            let migration_runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(_) => return 0,
            };
            let migration_result = migration_runtime.block_on(async {
                migrator
                    .create_pre_migration_backup(&database_pool, &product.database_path)
                    .await?;
                migrator.apply_pending(&database_pool).await?;
                Ok::<(), mukei_core::MukeiError>(())
            });
            if let Err(error) = migration_result {
                mukei_core::diagnostics::logger::log_error(&error);
                return 0;
            }

            let services = super::super::runtime_services(&config);
            let runtime = match MukeiRuntime::create_with_services(config, services) {
                Ok(runtime) => Arc::new(runtime),
                Err(_) => return 0,
            };
            let handle = match super::super::RUNTIMES.lock().insert(Arc::clone(&runtime)) {
                Some(handle) => handle,
                None => {
                    runtime.shutdown();
                    return 0;
                }
            };
            SECURE_RESOURCES.lock().insert(
                handle,
                SecureResources {
                    _database: database_pool,
                    encryption_status,
                },
            );
            handle
        }))
        .unwrap_or(0)
    }

    #[no_mangle]
    pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_securityStatus(
        mut env: JNIEnv<'_>,
        _this: JObject<'_>,
        handle: jlong,
    ) -> jbyteArray {
        let response = super::super::guarded_bytes(|| {
            if super::super::runtime_entry(handle).is_none() {
                return super::super::invalid_handle_payload();
            }
            let resources = SECURE_RESOURCES.lock();
            let Some(resources) = resources.get(&handle) else {
                return super::super::serialize(&json!({
                    "sqlcipher": "not_configured",
                    "panic_hook": mukei_core::diagnostics::panic_hook::is_installed(),
                }));
            };
            super::super::serialize(&json!({
                "sqlcipher": status_tag(resources.encryption_status),
                "panic_hook": mukei_core::diagnostics::panic_hook::is_installed(),
                "crash_sink": "app_private",
                "telemetry": "local_only",
            }))
        });
        super::super::to_java_bytes(&mut env, &response)
    }

    #[no_mangle]
    pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_destroySecureRuntime(
        _env: JNIEnv<'_>,
        _this: JObject<'_>,
        handle: jlong,
    ) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            SECURE_RESOURCES.lock().remove(&handle);
            let runtime = super::super::RUNTIMES.lock().remove(handle);
            if let Some(runtime) = runtime {
                runtime.shutdown();
            }
        }));
    }

    #[cfg(test)]
    mod tests {
        use super::app_private_root;

        #[test]
        fn app_private_root_requires_absolute_non_traversing_path() {
            assert!(app_private_root("/data/user/0/ai.mukei.android/files/mukei").is_ok());
            assert!(app_private_root("relative/mukei").is_err());
            assert!(app_private_root("/data/user/0/../escape").is_err());
        }
    }
}

#[cfg(not(feature = "secure_runtime"))]
mod secure_runtime {
    use jni::objects::{JByteArray, JObject};
    use jni::sys::{jbyteArray, jlong};
    use jni::JNIEnv;

    #[no_mangle]
    pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_createSecureRuntime(
        _env: JNIEnv<'_>,
        _this: JObject<'_>,
        _config_json: JByteArray<'_>,
        _database_key: JByteArray<'_>,
    ) -> jlong {
        0
    }

    #[no_mangle]
    pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_securityStatus(
        mut env: JNIEnv<'_>,
        _this: JObject<'_>,
        _handle: jlong,
    ) -> jbyteArray {
        let payload = super::super::error_payload(
            "secure_runtime_unavailable",
            "This native library was not built with the secure_runtime feature.",
        );
        super::super::to_java_bytes(&mut env, &payload)
    }

    #[no_mangle]
    pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_destroySecureRuntime(
        _env: JNIEnv<'_>,
        _this: JObject<'_>,
        _handle: jlong,
    ) {
    }
}
