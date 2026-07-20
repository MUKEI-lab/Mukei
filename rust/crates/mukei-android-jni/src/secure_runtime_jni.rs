use jni::objects::JObject;
use jni::sys::jbyteArray;
use jni::JNIEnv;
use zeroize::Zeroize;

use std::panic::{catch_unwind, AssertUnwindSafe};

fn generate_32_byte_key(env: &mut JNIEnv<'_>) -> jbyteArray {
    let mut key = [0_u8; 32];
    let result = catch_unwind(AssertUnwindSafe(|| {
        getrandom::getrandom(&mut key).map_err(|_| ())?;
        Ok::<_, ()>(super::to_java_bytes(env, &key))
    }))
    .ok()
    .and_then(Result::ok)
    .unwrap_or(std::ptr::null_mut());
    key.zeroize();
    result
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_generateDatabaseKey(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jbyteArray {
    generate_32_byte_key(&mut env)
}

#[no_mangle]
pub extern "system" fn Java_ai_mukei_android_core_nativebridge_NativeBindings_generateObjectStoreKey(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jbyteArray {
    generate_32_byte_key(&mut env)
}

#[cfg(feature = "secure_runtime")]
mod secure_runtime {
    use std::collections::HashMap;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::path::{Component, Path, PathBuf};
    use std::sync::Arc;

    use async_trait::async_trait;
    use jni::objects::{JByteArray, JObject};
    use jni::sys::{jbyteArray, jlong};
    use jni::JNIEnv;
    use mukei_core::application_runtime::{MukeiRuntime, RuntimeConfig, RuntimeProjectionStore};
    use mukei_core::diagnostics::{install_panic_hook, CrashFingerprint, CrashSink, PanicSink};
    use mukei_core::storage::{
        Aes256GcmObjectCipher, DatabaseEncryptionStatus, DatabasePool, ImmutableObjectStore,
        Migrator, RuntimeProjectionRepository, SqlStorageWorkspaceService, StagedFileImporter,
        StagedPlaintextCleanup, StorageWorkspacePort, WorkspaceStagedImportService,
        DEFAULT_MAX_STAGED_IMPORT_BYTES,
    };
    use once_cell::sync::Lazy;
    use parking_lot::Mutex;
    use serde_json::{json, Value};
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

    struct SqlcipherProjectionStore {
        pool: Arc<DatabasePool>,
    }

    #[async_trait]
    impl RuntimeProjectionStore for SqlcipherProjectionStore {
        async fn load(&self, key: &str) -> Result<Option<Value>, mukei_core::MukeiError> {
            let rows = RuntimeProjectionRepository::list_domain(&self.pool, "runtime").await?;
            let Some(row) = rows.into_iter().find(|row| row.projection_key == key) else {
                return Ok(None);
            };
            serde_json::from_str(&row.payload_json)
                .map(Some)
                .map_err(|_| mukei_core::MukeiError::DatabaseCorruption)
        }

        async fn save(&self, key: &str, value: Value) -> Result<(), mukei_core::MukeiError> {
            let payload = serde_json::to_string(&value)
                .map_err(|error| mukei_core::MukeiError::Internal(error.to_string()))?;
            RuntimeProjectionRepository::upsert(&self.pool, "runtime", key, payload).await
        }

        async fn delete(&self, key: &str) -> Result<(), mukei_core::MukeiError> {
            RuntimeProjectionRepository::delete(&self.pool, "runtime", key).await
        }
    }

    struct SecureResources {
        _database: Arc<DatabasePool>,
        encryption_status: DatabaseEncryptionStatus,
        object_store_ready: bool,
        staged_cleanup_removed: usize,
        staged_cleanup_unsafe_paths: usize,
        rag_ready: bool,
    }

    static SECURE_RESOURCES: Lazy<Mutex<HashMap<jlong, SecureResources>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));
    fn app_private_root(app_data_dir: &str) -> Result<PathBuf, ()> {
        let root = PathBuf::from(app_data_dir);
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
        env: JNIEnv<'_>,
        _this: JObject<'_>,
        config_json: JByteArray<'_>,
        database_key: JByteArray<'_>,
        object_store_key: JByteArray<'_>,
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
            if install_security_boundary(&app_data_root).is_err() {
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
            let mut object_key_bytes = match env.convert_byte_array(&object_store_key) {
                Ok(bytes) => bytes,
                Err(_) => {
                    key_bytes.zeroize();
                    return 0;
                }
            };
            if object_key_bytes.len() != 32 {
                key_bytes.zeroize();
                object_key_bytes.zeroize();
                return 0;
            }
            let object_key: [u8; 32] = match object_key_bytes.as_slice().try_into() {
                Ok(value) => value,
                Err(_) => {
                    key_bytes.zeroize();
                    object_key_bytes.zeroize();
                    return 0;
                }
            };
            object_key_bytes.zeroize();
            let object_cipher = Aes256GcmObjectCipher::new(object_key);

            let config_path = app_data_root.join("mukei.toml");
            if !config_path.is_file() && mukei_core::config::write_default(&config_path).is_err() {
                key_bytes.zeroize();
                return 0;
            }
            let product = match mukei_core::config::MukeiConfig::load_and_validate(&config_path) {
                Ok(value) => value,
                Err(error) => {
                    key_bytes.zeroize();
                    mukei_core::diagnostics::logger::log_error(&error);
                    return 0;
                }
            };
            if product
                .validate_android_storage_paths(&config_path)
                .is_err()
                || product.ensure_storage_directories().is_err()
            {
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
            if encryption_status != DatabaseEncryptionStatus::Encrypted {
                return 0;
            }
            let database_pool = Arc::new(database.pool);
            let migration_runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(_) => return 0,
            };
            let migration_result = migration_runtime.block_on(async {
                let migrator = Migrator::embedded();
                migrator
                    .create_pre_migration_backup(&database_pool, &product.database_path)
                    .await?;
                migrator.apply_pending(&database_pool).await?;
                RuntimeProjectionRepository::ensure_schema(&database_pool).await?;
                Ok::<(), mukei_core::MukeiError>(())
            });
            if let Err(error) = migration_result {
                mukei_core::diagnostics::logger::log_error(&error);
                return 0;
            }
            let staging_root = app_data_root.join("documents");
            let cleanup_report = match migration_runtime.block_on(
                StagedPlaintextCleanup::sweep_terminal(&database_pool, &staging_root),
            ) {
                Ok(report) => report,
                Err(error) => {
                    mukei_core::diagnostics::logger::log_error(&error);
                    return 0;
                }
            };
            let object_store =
                match ImmutableObjectStore::open(app_data_root.join("objects"), object_cipher) {
                    Ok(store) => Arc::new(store),
                    Err(error) => {
                        tracing::error!(code = "object_store_open_failed", error = %error);
                        return 0;
                    }
                };
            let importer: Arc<dyn StagedFileImporter> = match WorkspaceStagedImportService::new(
                Arc::clone(&database_pool),
                object_store,
                staging_root,
                DEFAULT_MAX_STAGED_IMPORT_BYTES,
            ) {
                Ok(service) => Arc::new(service),
                Err(error) => {
                    tracing::error!(code = error.code(), "workspace importer unavailable");
                    return 0;
                }
            };

            let storage_workspace: Arc<dyn StorageWorkspacePort> =
                Arc::new(SqlStorageWorkspaceService::new(Arc::clone(&database_pool)));
            let mut services = crate::runtime_services(&config);
            services.storage_importer = Some(importer);
            services.storage_workspace = Some(storage_workspace);
            let runtime = match MukeiRuntime::create_with_services(config, services) {
                Ok(runtime) => Arc::new(runtime),
                Err(_) => return 0,
            };
            let projection_store: Arc<dyn RuntimeProjectionStore> =
                Arc::new(SqlcipherProjectionStore {
                    pool: Arc::clone(&database_pool),
                });
            if let Err(error) = runtime.attach_projection_store(projection_store) {
                mukei_core::diagnostics::logger::log_error(&error);
                runtime.shutdown();
                return 0;
            }

            #[cfg(feature = "rag_runtime")]
            let rag_ready = match crate::native_rag::AndroidRagService::open(
                &app_data_root,
                Arc::clone(&database_pool),
            ) {
                Ok(service) => {
                    runtime.attach_rag_service(service);
                    true
                }
                Err(error) => {
                    tracing::info!(
                        code = error.error_code(),
                        "verified embedding bundle unavailable; RAG capability disabled"
                    );
                    false
                }
            };
            #[cfg(not(feature = "rag_runtime"))]
            let rag_ready = false;

            let handle = match crate::RUNTIMES.lock().insert(Arc::clone(&runtime)) {
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
                    object_store_ready: true,
                    staged_cleanup_removed: cleanup_report.removed,
                    staged_cleanup_unsafe_paths: cleanup_report.unsafe_paths,
                    rag_ready,
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
                "projections": "encrypted",
                "object_store": if resources.object_store_ready { "encrypted" } else { "unavailable" },
                "staged_plaintext_cleanup": {
                    "removed": resources.staged_cleanup_removed,
                    "unsafe_paths": resources.staged_cleanup_unsafe_paths,
                },
                "rag": if resources.rag_ready { "ready" } else { "artifacts_required" },
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
            let runtime = crate::RUNTIMES.lock().remove(handle);
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
        _object_store_key: JByteArray<'_>,
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
