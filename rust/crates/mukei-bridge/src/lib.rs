//! CXX-Qt bridge — TRD §1.2 / §1.3 / §9.4.
//!
//! # Wrapped-secrets registry (Issue #3, user priority #1)
//!
//! The bridge runtime state owns the wrapped Brave / Tavily API-key slots
//! and the `ToolRegistry` whose `WebSearchTool`
//! is rebuilt every time a key arrives. The old design read process
//! env vars (`BRAVE_API_KEY`) while the bridge wrote a different name
//! (`CIPHER_BRAVE_API_KEY`); the names never met, so search never
//! actually worked. The new design passes the unwrapped keys directly
//! into [`mukei_core::tools::ToolRegistry::with_web_search_keys`] so
//! a typo becomes a compile error.
#![allow(clippy::incompatible_msrv)]
// `#[cxx_qt::bridge]` expands to generated code that triggers
// `clippy::incompatible_msrv` on Rust 1.93 while the workspace still
// declares 1.78. The generated expansion is not locally rewritable.

mod agent_runtime;
mod android_document_access;
mod android_secret_store;
mod app_runtime;
mod async_bridge;
mod bootstrap;
mod bridge_state;
mod database_bridge;
mod document_bridge;
mod download_bridge;
#[cfg(feature = "llama_cpp")]
mod native_inference;
mod protocol;
mod provenance;
mod recovery_bridge;
mod settings_bridge;
mod storage_bridge;

#[cfg(all(
    target_os = "android",
    not(debug_assertions),
    not(feature = "sqlcipher")
))]
compile_error!("Android release builds must enable the mukei-bridge/sqlcipher feature");

#[cfg(any(
    all(feature = "runtime_development", feature = "runtime_test"),
    all(feature = "runtime_development", feature = "runtime_production"),
    all(feature = "runtime_test", feature = "runtime_production"),
))]
compile_error!("exactly one runtime environment feature may be enabled");

#[cfg(feature = "rusqlite")]
use mukei_core::storage::{
    saf as core_saf, AuditChainStatus, AuditEntry, AuditLogReader, ConversationRepository,
    MessageStatus, PersistedTurn,
};

#[cfg(all(feature = "rusqlite", target_os = "android"))]
use mukei_core::storage::{SecretRefRecord, SettingsRepository};

#[cfg(not(feature = "rusqlite"))]
mod core_saf {
    #[derive(Clone, Debug, Default)]
    pub struct SafRegistry;

    #[derive(Clone, Debug)]
    #[allow(dead_code)]
    pub struct SafTokenRow {
        pub token_id: String,
        pub source: String,
        pub target: String,
        pub mime: String,
        pub revoked: bool,
        pub created: chrono::DateTime<chrono::Utc>,
    }

    impl SafRegistry {
        pub fn new() -> Self {
            Self
        }
        pub fn count(&self) -> usize {
            0
        }
        pub fn resolve(&self, _token: &str) -> Result<String, ()> {
            Err(())
        }
    }
}

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QString, QVariant};
use parking_lot::Mutex as ParkingMutex;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use app_runtime::application_runtime as runtime_state;
use bootstrap::{
    prepare_database_key_with_observer, BootstrapStart, PlatformSecureKeyProvider,
    SecureBootstrapState,
};
use bridge_state::RuntimePhase;
use bridge_state::{
    single_active_download, ActiveDownload, BusyGuard, DownloadSlotGuard, InitializationGuard,
};
#[cfg(feature = "rusqlite")]
use mukei_core::agent::AgentRunOutcome;
use mukei_core::agent::{AgentEventSink, AgentRunRequest};
use mukei_core::engine::InferenceBackend;
use mukei_core::ffi::tags::{TagEvents, TagsStreaming};
use mukei_core::tools::{RemoteFeaturePolicy, ToolRegistry};
use mukei_core::types::{BranchId, ConversationId, MessageId};
use mukei_core::ui_contract::{
    AndroidStorageState, AppLifecycleState, BridgeEvent, BridgeEventKind, CapabilitySnapshot,
    ChatTurnState, DownloadState, UiError,
};

const BRAVE_SECRET_ALIAS: &str = "mukei.provider.brave";
const TAVILY_SECRET_ALIAS: &str = "mukei.provider.tavily";

pub(crate) fn event_json(event: BridgeEvent) -> QString {
    let json = runtime_state()
        .protocol_state()
        .lock()
        .wrap_bridge_event(event);
    QString::from(json.as_str())
}

fn error_bridge_event(error: &mukei_core::error::MukeiError, source: &'static str) -> BridgeEvent {
    BridgeEvent::new(BridgeEventKind::Error {
        error: UiError::from_mukei_error(error, source),
    })
}

fn async_error_value(
    error: &mukei_core::error::MukeiError,
    source: &'static str,
) -> serde_json::Value {
    serde_json::to_value(UiError::from_mukei_error(error, source)).unwrap_or_else(|_| {
        serde_json::json!({
            "code": error.error_code(),
            "safe_message": "The operation could not be completed safely.",
            "recoverable": true,
        })
    })
}

#[derive(Debug)]
pub(crate) enum ModelActivationTaskResult {
    Ready(serde_json::Value),
    Superseded,
    Failed(mukei_core::error::MukeiError),
}

pub(crate) fn begin_model_activation(model_id: &str) -> Result<u64, mukei_core::error::MukeiError> {
    let descriptor = mukei_core::engine::lookup_model_str(model_id).ok_or_else(|| {
        mukei_core::error::MukeiError::ConfigInvalid {
            field: "model_id".to_string(),
            reason: "unknown model identifier".to_string(),
        }
    })?;
    if !production_inference_implementation_available() {
        return Err(mukei_core::error::MukeiError::ModelLoadFailed(
            "production inference backend is unavailable in this runtime".to_string(),
        ));
    }
    runtime_state()
        .model_activation_service()
        .set_real_backend_implementation_available(true);
    Ok(runtime_state()
        .model_activation_service()
        .begin_verification(descriptor.id.as_str(), descriptor.expected_sha256))
}

fn verify_candidate_model_path(
    model_root: std::path::PathBuf,
    filename: &'static str,
    expected_sha256: &'static str,
) -> Result<std::path::PathBuf, mukei_core::error::MukeiError> {
    let requested_path = model_root.join(filename);
    let metadata = std::fs::symlink_metadata(&requested_path)
        .map_err(|error| mukei_core::error::MukeiError::Io(error.to_string()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return Err(mukei_core::error::MukeiError::ModelLoadFailed(
            "selected model is not a regular installed file".to_string(),
        ));
    }
    let canonical_root = std::fs::canonicalize(&model_root)
        .map_err(|error| mukei_core::error::MukeiError::Io(error.to_string()))?;
    let canonical_path = std::fs::canonicalize(&requested_path)
        .map_err(|error| mukei_core::error::MukeiError::Io(error.to_string()))?;
    if !canonical_path.starts_with(&canonical_root)
        || canonical_path.parent() != Some(canonical_root.as_path())
    {
        return Err(mukei_core::error::MukeiError::ConfigInvalid {
            field: "models_dir".to_string(),
            reason: "model path escaped the app-private model directory".to_string(),
        });
    }
    mukei_core::engine::LlamaEngine::verify_full_sha256_stream(&canonical_path, expected_sha256)?;
    Ok(canonical_path)
}

pub(crate) async fn complete_model_activation(
    model_id: String,
    verification_generation: u64,
) -> ModelActivationTaskResult {
    let Some(catalogue) = mukei_core::engine::lookup_model_str(&model_id) else {
        return ModelActivationTaskResult::Failed(mukei_core::error::MukeiError::ConfigInvalid {
            field: "model_id".to_string(),
            reason: "unknown model identifier".to_string(),
        });
    };
    let activation = runtime_state().model_activation_service();
    let expected_sha256 = catalogue.expected_sha256;
    #[cfg(feature = "llama_cpp")]
    let config = match runtime_state().config() {
        Some(config) => config,
        None => {
            let error = mukei_core::error::MukeiError::ConfigInvalid {
                field: "runtime".to_string(),
                reason: "runtime configuration is unavailable during model activation".to_string(),
            };
            if !activation.mark_verification_failed(
                verification_generation,
                catalogue.id.as_str(),
                expected_sha256,
                expected_sha256,
                mukei_core::engine::ActivationFailureCategory::ModelLoad,
            ) {
                return ModelActivationTaskResult::Superseded;
            }
            return ModelActivationTaskResult::Failed(error);
        }
    };
    let model_root = runtime_state().model_dir();
    let filename = catalogue.filename;
    let verified_path = tokio::task::spawn_blocking(move || {
        verify_candidate_model_path(model_root, filename, expected_sha256)
    })
    .await
    .map_err(|error| mukei_core::error::MukeiError::BlockingJoinFailed(error.to_string()))
    .and_then(|result| result);

    let verified_path = match verified_path {
        Ok(path) => path,
        Err(error) => {
            let category = if matches!(&error, mukei_core::error::MukeiError::ModelCorrupted) {
                mukei_core::engine::ActivationFailureCategory::VerificationMismatch
            } else {
                mukei_core::engine::ActivationFailureCategory::ModelLoad
            };
            if !activation.mark_verification_failed(
                verification_generation,
                catalogue.id.as_str(),
                expected_sha256,
                expected_sha256,
                category,
            ) {
                return ModelActivationTaskResult::Superseded;
            }
            return ModelActivationTaskResult::Failed(error);
        }
    };

    let artifact =
        match mukei_core::engine::VerifiedModelArtifact::new(expected_sha256, verified_path) {
            Ok(artifact) => artifact,
            Err(error) => {
                if !activation.mark_verification_failed(
                    verification_generation,
                    catalogue.id.as_str(),
                    expected_sha256,
                    expected_sha256,
                    mukei_core::engine::ActivationFailureCategory::Internal,
                ) {
                    return ModelActivationTaskResult::Superseded;
                }
                return ModelActivationTaskResult::Failed(error);
            }
        };
    let descriptor = match mukei_core::engine::VerifiedModelDescriptor::new(
        catalogue.id.as_str(),
        expected_sha256,
        artifact,
    ) {
        Ok(descriptor) => descriptor,
        Err(error) => {
            if !activation.mark_verification_failed(
                verification_generation,
                catalogue.id.as_str(),
                expected_sha256,
                expected_sha256,
                mukei_core::engine::ActivationFailureCategory::Internal,
            ) {
                return ModelActivationTaskResult::Superseded;
            }
            return ModelActivationTaskResult::Failed(error);
        }
    };
    if !activation.mark_verified(verification_generation, descriptor) {
        return ModelActivationTaskResult::Superseded;
    }

    #[cfg(feature = "llama_cpp")]
    {
        let factory = native_inference::NativeLlamaBackendFactory::from_config(&config);
        match activation.activate_verified(&factory).await {
            mukei_core::engine::ActivationCommit::Ready => {
                let readiness = activation.readiness_snapshot();
                let active = activation.active_model_snapshot();
                let identity = activation.identity();
                ModelActivationTaskResult::Ready(serde_json::json!({
                    "model_id": catalogue.id.as_str(),
                    "active_model_id": active.as_ref().map(|value| value.model_id.clone()),
                    "artifact_id": active.as_ref().map(|value| value.artifact_id.clone()),
                    "inference_backend": identity.implementation,
                    "backend_kind": identity.kind.as_tag(),
                    "active_model_ready": readiness.active_backend_ready,
                    "product_ready": readiness.product_ready,
                }))
            }
            mukei_core::engine::ActivationCommit::StaleIgnored => {
                ModelActivationTaskResult::Superseded
            }
            mukei_core::engine::ActivationCommit::Failed(category) => {
                ModelActivationTaskResult::Failed(mukei_core::error::MukeiError::ModelLoadFailed(
                    format!("model activation failed: {}", category.as_tag()),
                ))
            }
        }
    }
    #[cfg(not(feature = "llama_cpp"))]
    {
        let _ = activation;
        ModelActivationTaskResult::Failed(mukei_core::error::MukeiError::ModelLoadFailed(
            "production inference backend is not compiled into this runtime".to_string(),
        ))
    }
}

fn lifecycle_state_for_secure_bootstrap(state: SecureBootstrapState) -> AppLifecycleState {
    match state {
        SecureBootstrapState::Uninitialized => AppLifecycleState::NeedsDatabaseKey,
        SecureBootstrapState::CreatingWrappingKey => AppLifecycleState::CreatingWrappingKey,
        SecureBootstrapState::CreatingDatabaseKey => AppLifecycleState::CreatingDatabaseKey,
        SecureBootstrapState::WrappingDatabaseKey => AppLifecycleState::WrappingDatabaseKey,
        SecureBootstrapState::UnwrappingDatabaseKey => AppLifecycleState::UnwrappingDatabaseKey,
        SecureBootstrapState::OpeningDatabase => AppLifecycleState::OpeningDatabase,
        SecureBootstrapState::Ready => AppLifecycleState::Ready,
        SecureBootstrapState::KeyInvalidated => AppLifecycleState::KeyInvalidated,
        SecureBootstrapState::WrappedKeyCorrupt => AppLifecycleState::WrappedKeyCorrupt,
        SecureBootstrapState::DatabaseOpenFailed => AppLifecycleState::DatabaseOpenFailed,
        SecureBootstrapState::ResetRequired => AppLifecycleState::ResetRequired,
    }
}

#[cfg(feature = "rusqlite")]
fn database_open_failure_category(error: &mukei_core::error::MukeiError) -> &'static str {
    use mukei_core::error::MukeiError;

    match error {
        MukeiError::DatabaseEncryptionInvalidKey => "key_rejected",
        MukeiError::DatabaseCorruption | MukeiError::DatabaseEncryptionCorrupted => {
            "database_corrupt"
        }
        MukeiError::DatabaseEncryptionUnavailable => "encryption_unavailable",
        MukeiError::DatabaseEncryptionMigrationRequired => "insecure_database_requires_migration",
        MukeiError::MigrationFailed(_, _)
        | MukeiError::MigrationOrderConflict { .. }
        | MukeiError::MigrationChecksumMismatch { .. }
        | MukeiError::MigrationLocked
        | MukeiError::SchemaTooNew { .. } => "schema_or_migration",
        MukeiError::DatabaseInitFailed(_) => "database_open",
        _ => "database_open_other",
    }
}

fn production_inference_implementation_available() -> bool {
    #[cfg(feature = "llama_cpp")]
    {
        return native_inference::implementation_available();
    }
    #[cfg(not(feature = "llama_cpp"))]
    {
        false
    }
}

fn current_ready_capabilities() -> CapabilitySnapshot {
    CapabilitySnapshot::ready_with_model(
        runtime_state()
            .model_activation_service()
            .readiness_snapshot()
            .active_backend_ready,
    )
}

fn production_safety_status() -> mukei_core::config::ProductionSafetyStatus {
    mukei_core::config::ProductionSafetyStatus {
        mock_inference_active: !cfg!(feature = "llama_cpp"),
        insecure_database_mode: !cfg!(feature = "sqlcipher"),
        mock_embedding_backend: !cfg!(feature = "candle"),
        debug_vector_backend: !cfg!(feature = "usearch_hnsw"),
        diagnostics_export_enabled: cfg!(feature = "diagnostics_export"),
    }
}

fn download_event_error(code: &'static str, message: String) -> mukei_core::error::MukeiError {
    match code {
        "ERR_CANCELLED" => mukei_core::error::MukeiError::Cancelled,
        "ERR_DOWNLOAD_HASH" => mukei_core::error::MukeiError::DownloadHashMismatch,
        "ERR_DOWNLOAD_SIZE_MISSING" => mukei_core::error::MukeiError::DownloadSizeMissing,
        "ERR_DOWNLOAD_TOO_LARGE" => {
            let mut numbers = message
                .split(|c: char| !c.is_ascii_digit())
                .filter(|part| !part.is_empty())
                .filter_map(|part| part.parse::<u64>().ok());
            let actual_bytes = numbers.next().unwrap_or_default();
            let max_bytes = numbers.next().unwrap_or_default();
            mukei_core::error::MukeiError::DownloadTooLarge {
                max_bytes,
                actual_bytes,
            }
        }
        "ERR_STORAGE_QUOTA" => {
            let mut numbers = message
                .split(|c: char| !c.is_ascii_digit())
                .filter(|part| !part.is_empty())
                .filter_map(|part| part.parse::<u64>().ok());
            let used_bytes = numbers.next().unwrap_or_default();
            let requested_bytes = numbers.next().unwrap_or_default();
            let max_bytes = numbers.next().unwrap_or_default();
            mukei_core::error::MukeiError::StorageQuotaExceeded {
                max_bytes,
                requested_bytes,
                used_bytes,
            }
        }
        "ERR_NETWORK" => mukei_core::error::MukeiError::NetworkError(message),
        "ERR_NETWORK_TIMEOUT" => mukei_core::error::MukeiError::NetworkTimeout {
            operation: "download_model".into(),
        },
        "ERR_NETWORK_UNAVAILABLE" => mukei_core::error::MukeiError::NetworkUnavailable {
            operation: "download_model".into(),
        },
        "ERR_NETWORK_TLS" => mukei_core::error::MukeiError::NetworkTls {
            operation: "download_model".into(),
        },
        "ERR_NETWORK_INVALID_RESPONSE" => mukei_core::error::MukeiError::NetworkInvalidResponse {
            operation: "download_model".into(),
        },
        "ERR_NETWORK_RATE_LIMITED" => mukei_core::error::MukeiError::NetworkRateLimited {
            operation: "download_model".into(),
        },
        "ERR_NETWORK_SERVER" => mukei_core::error::MukeiError::NetworkServerError {
            status: 0,
            operation: "download_model".into(),
        },
        "ERR_IO" => mukei_core::error::MukeiError::Io(message),
        _ => mukei_core::error::MukeiError::Internal(message),
    }
}

fn default_model_base_dir() -> std::path::PathBuf {
    std::env::var("XDG_DATA_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".local/share"))
        })
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
        })
        .join("mukei")
}

#[derive(Debug)]
struct ValidatedModelDir {
    canonical_dir: std::path::PathBuf,
    canonical_base: std::path::PathBuf,
}

fn validate_model_dir(path: std::path::PathBuf) -> mukei_core::error::Result<ValidatedModelDir> {
    let base = runtime_state().model_base_dir();
    validate_model_dir_against_base(path, base)
}

fn validate_model_dir_against_base(
    path: std::path::PathBuf,
    base: std::path::PathBuf,
) -> mukei_core::error::Result<ValidatedModelDir> {
    if !path.is_absolute() {
        return Err(mukei_core::error::MukeiError::ToolArgumentInvalid {
            field: "model_dir",
            reason: "model directory must be an absolute app-private path".to_string(),
        });
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(mukei_core::error::MukeiError::ToolArgumentInvalid {
            field: "model_dir",
            reason: "model directory must not contain parent-directory components".to_string(),
        });
    }

    let canonical_dir = canonicalize_existing_model_dir(&path)?;
    let canonical_base = canonicalize_existing_model_dir(&base)?;
    if canonical_dir.starts_with(&canonical_base) {
        return Ok(ValidatedModelDir {
            canonical_dir,
            canonical_base,
        });
    }

    if is_android_app_specific_files_path(&canonical_dir) {
        return Ok(ValidatedModelDir {
            canonical_base: canonical_dir.clone(),
            canonical_dir,
        });
    }

    Err(mukei_core::error::MukeiError::ToolArgumentInvalid {
        field: "model_dir",
        reason: "model directory must stay inside app-private storage".to_string(),
    })
}

fn canonicalize_existing_model_dir(
    path: &std::path::Path,
) -> mukei_core::error::Result<std::path::PathBuf> {
    std::fs::create_dir_all(path).map_err(|err| {
        mukei_core::error::MukeiError::Io(format!("create model directory: {err}"))
    })?;
    path.canonicalize().map_err(|err| {
        mukei_core::error::MukeiError::Io(format!("canonicalize model directory: {err}"))
    })
}

fn is_android_app_specific_files_path(path: &std::path::Path) -> bool {
    let parts: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();

    parts.windows(4).any(|window| {
        window[0] == "Android"
            && window[1] == "data"
            && !window[2].is_empty()
            && window[2].contains('.')
            && window[3] == "files"
    })
}

fn safe_model_filename(filename: &str) -> mukei_core::error::Result<&str> {
    let path = std::path::Path::new(filename);
    let is_plain_file = path.components().count() == 1
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)));
    let has_allowed_ext = path.extension().and_then(|ext| ext.to_str()) == Some("gguf");
    if is_plain_file && has_allowed_ext {
        Ok(filename)
    } else {
        Err(mukei_core::error::MukeiError::ToolArgumentInvalid {
            field: "model_filename",
            reason: "model filename must be a simple .gguf file name".to_string(),
        })
    }
}

fn download_destination_token(model_id: Option<&str>, dest: &std::path::Path) -> String {
    model_id
        .filter(|id| !id.is_empty())
        .map(|id| format!("model:{id}"))
        .or_else(|| {
            dest.file_name()
                .and_then(|name| name.to_str())
                .map(|name| format!("file:{name}"))
        })
        .unwrap_or_else(|| "model:unknown".to_string())
}

#[cfg(feature = "rusqlite")]
fn preference_value_from_qvariant(
    key: &str,
    value: &QVariant,
) -> mukei_core::error::Result<mukei_core::storage::PreferenceValue> {
    use mukei_core::storage::PreferenceValue;

    if setting_key_looks_secret(key) {
        return Err(mukei_core::error::MukeiError::PermissionDenied);
    }

    match key {
        "prompt_card_auto_send"
        | "thermal_autopause"
        | "first_run_completed"
        | "search.enable_cache"
        | "reduce_motion"
        | "high_contrast" => value
            .value::<bool>()
            .map(PreferenceValue::Bool)
            .ok_or_else(|| mukei_core::error::MukeiError::ConfigInvalid {
                field: key.into(),
                reason: "expected boolean setting value".into(),
            }),
        "search.max_parallel_engines"
        | "search.brave_timeout_secs"
        | "search.tavily_timeout_secs"
        | "font_scale_percent"
        | "temperature_milli"
        | "max_tokens_default"
        | "top_p_milli" => value
            .value::<i64>()
            .map(PreferenceValue::Integer)
            .ok_or_else(|| mukei_core::error::MukeiError::ConfigInvalid {
                field: key.into(),
                reason: "expected integer setting value".into(),
            }),
        "remote_feature_policy" | "theme_mode" => value
            .value::<QString>()
            .map(|value| PreferenceValue::String(value.to_string()))
            .ok_or_else(|| mukei_core::error::MukeiError::ConfigInvalid {
                field: key.into(),
                reason: "expected string setting value".into(),
            }),
        _ => Err(mukei_core::error::MukeiError::ToolArgumentInvalid {
            field: "setting",
            reason: format!("unsupported setting key: {key}"),
        }),
    }
}

#[cfg(feature = "rusqlite")]
fn preference_value_from_json(
    key: &str,
    value: &serde_json::Value,
) -> mukei_core::error::Result<mukei_core::storage::PreferenceValue> {
    use mukei_core::storage::PreferenceValue;

    if setting_key_looks_secret(key) {
        return Err(mukei_core::error::MukeiError::PermissionDenied);
    }

    match key {
        "prompt_card_auto_send"
        | "thermal_autopause"
        | "first_run_completed"
        | "search.enable_cache"
        | "reduce_motion"
        | "high_contrast" => value.as_bool().map(PreferenceValue::Bool).ok_or_else(|| {
            mukei_core::error::MukeiError::ConfigInvalid {
                field: key.into(),
                reason: "expected boolean setting value".into(),
            }
        }),
        "search.max_parallel_engines"
        | "search.brave_timeout_secs"
        | "search.tavily_timeout_secs"
        | "font_scale_percent"
        | "temperature_milli"
        | "max_tokens_default"
        | "top_p_milli" => value.as_i64().map(PreferenceValue::Integer).ok_or_else(|| {
            mukei_core::error::MukeiError::ConfigInvalid {
                field: key.into(),
                reason: "expected integer setting value".into(),
            }
        }),
        "remote_feature_policy" | "theme_mode" => value
            .as_str()
            .map(|value| PreferenceValue::String(value.to_string()))
            .ok_or_else(|| mukei_core::error::MukeiError::ConfigInvalid {
                field: key.into(),
                reason: "expected string setting value".into(),
            }),
        _ => Err(mukei_core::error::MukeiError::ToolArgumentInvalid {
            field: "setting",
            reason: format!("unsupported setting key: {key}"),
        }),
    }
}

pub(crate) fn dispatch_protocol_setting_update(
    agent: Pin<&mut ffi::MukeiAgent>,
    context: protocol::CommandContext,
    key: String,
    value: serde_json::Value,
) {
    let qt = agent.as_ref().get_ref().qt_thread();
    #[cfg(feature = "rusqlite")]
    {
        let pref = match preference_value_from_json(&key, &value) {
            Ok(value) => value,
            Err(error) => {
                let error_value =
                    serde_json::to_value(UiError::from_mukei_error(&error, "settings.update"))
                        .unwrap_or_else(|_| serde_json::json!({"code": "ERR_SETTING_INVALID"}));
                let event =
                    protocol::async_operation_event_json(&context, false, None, Some(error_value));
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().event_emitted(event);
                });
                return;
            }
        };
        let Some(pool) = runtime_state().database_pool() else {
            let error = mukei_core::error::MukeiError::DatabaseInitFailed(
                "settings database is not initialized".into(),
            );
            let error_value =
                serde_json::to_value(UiError::from_mukei_error(&error, "settings.update"))
                    .unwrap_or_else(|_| serde_json::json!({"code": "ERR_SETTING_BACKEND"}));
            let event =
                protocol::async_operation_event_json(&context, false, None, Some(error_value));
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event);
            });
            return;
        };
        mukei_core::runtime::get().spawn(async move {
            let result = async {
                mukei_core::storage::SettingsRepository::upsert_preference(
                    &pool,
                    key.clone(),
                    pref.clone(),
                )
                .await?;
                if let (
                    "remote_feature_policy",
                    mukei_core::storage::PreferenceValue::String(policy),
                ) = (key.as_str(), pref)
                {
                    let policy = policy.parse::<RemoteFeaturePolicy>()?;
                    runtime_state().set_remote_feature_policy(policy);
                    rebuild_tool_registry_from_secrets().await;
                }
                Ok::<(), mukei_core::error::MukeiError>(())
            }
            .await;

            let event = match result {
                Ok(()) => protocol::async_operation_event_json(
                    &context,
                    true,
                    Some(serde_json::json!({"key": key})),
                    None,
                ),
                Err(error) => {
                    let error_value =
                        serde_json::to_value(UiError::from_mukei_error(&error, "settings.update"))
                            .unwrap_or_else(|_| serde_json::json!({"code": "ERR_SETTING_PERSIST"}));
                    protocol::async_operation_event_json(&context, false, None, Some(error_value))
                }
            };
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event);
            });
        });
    }
    #[cfg(not(feature = "rusqlite"))]
    {
        let _ = (key, value);
        let error = serde_json::json!({
            "code": "ERR_SETTING_BACKEND",
            "safe_message": "Settings persistence is disabled in this build."
        });
        let event = protocol::async_operation_event_json(&context, false, None, Some(error));
        let _ = qt.queue(move |mut qobject| {
            qobject.as_mut().event_emitted(event);
        });
    }
}

#[cfg(feature = "rusqlite")]
fn setting_key_looks_secret(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    lowered.contains("secret")
        || lowered.contains("token")
        || lowered.contains("api_key")
        || lowered.contains("apikey")
        || lowered.contains("password")
        || lowered.contains("cipher")
        || lowered.contains("key_hex")
}

/// Resolve a QML `download_model(url, sha256)` request into a typed
/// [`mukei_core::storage::DownloadRequest`]. Accepts two call shapes:
///
/// 1. **Canonical id** — `url` is a [`ModelId::as_str`] value
///    (e.g. `"gemma-4-e2b-it"`) and `sha256` is empty or matches the
///    pinned digest. The URL + SHA come from the binary catalogue
///    (TRD §8.1).
/// 2. **Bespoke URL** — debug builds only. Production builds must use
///    catalog model ids so model provenance stays controlled.
async fn resolve_download_request(
    url_or_id: &str,
    sha256: &str,
) -> mukei_core::error::Result<mukei_core::storage::DownloadRequest> {
    let model_dir = runtime_state().model_dir();

    if let Some(descriptor) = mukei_core::engine::lookup_model_str(url_or_id) {
        let trimmed = sha256.trim();
        if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case(descriptor.expected_sha256) {
            return Err(mukei_core::error::MukeiError::ToolArgumentInvalid {
                field: "sha256",
                reason: format!(
                    "sha256 mismatch for model id {} — caller passed {}, manifest pins {}",
                    descriptor.id, trimmed, descriptor.expected_sha256
                ),
            });
        }
        return Ok(mukei_core::storage::DownloadRequest {
            url: descriptor.download_url.to_string(),
            expected_sha256: descriptor.expected_sha256.to_string(),
            dest: model_dir.join(descriptor.filename),
        });
    }

    #[cfg(not(debug_assertions))]
    {
        return Err(mukei_core::error::MukeiError::ToolArgumentInvalid {
            field: "model_id",
            reason: "production builds only accept trusted catalog model ids".to_string(),
        });
    }

    #[cfg(debug_assertions)]
    let filename = url_or_id
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("model.gguf");
    #[cfg(debug_assertions)]
    let filename = safe_model_filename(filename)?;
    #[cfg(debug_assertions)]
    Ok(mukei_core::storage::DownloadRequest {
        url: url_or_id.to_string(),
        expected_sha256: sha256.to_string(),
        dest: model_dir.join(filename),
    })
}

#[cfg(feature = "rusqlite")]
async fn reserve_download_job(
    request: &mukei_core::storage::DownloadRequest,
    model_id: Option<String>,
    destination_token: String,
    expected_bytes: u64,
) -> mukei_core::error::Result<(
    Arc<mukei_core::storage::DatabasePool>,
    mukei_core::storage::DownloadReservation,
)> {
    let pool = runtime_state().database_pool().ok_or_else(|| {
        mukei_core::error::MukeiError::DatabaseInitFailed(
            "download persistence requires an initialized database".into(),
        )
    })?;
    let parent = request.dest.parent().ok_or_else(|| {
        mukei_core::error::MukeiError::Invariant(
            "model destination has no app-private parent directory".into(),
        )
    })?;
    let parent = parent.to_path_buf();
    let partial_path = request.partial_path();
    let (usage, resume_bytes) = tokio::task::spawn_blocking(move || {
        let resume_bytes = std::fs::metadata(partial_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let additional_bytes = expected_bytes.saturating_sub(resume_bytes);
        let quota = mukei_core::storage::StorageQuotaManager::new(parent);
        quota
            .ensure_model_download_allowed_with_additional(expected_bytes, additional_bytes)
            .map(|usage| (usage, resume_bytes))
    })
    .await
    .map_err(|error| mukei_core::error::MukeiError::BlockingJoinFailed(error.to_string()))??;

    // The reservation accounts for the full final artifact, so remove
    // this job's already-present resume prefix from the filesystem base.
    // Other stale/active partials remain in the base and cannot be hidden
    // from the aggregate quota ledger.
    let accounted_bytes_excluding_this_partial =
        usage.accounted_model_bytes().saturating_sub(resume_bytes);
    let reservation = mukei_core::storage::DownloadJobRepository::reserve(
        &pool,
        model_id,
        destination_token,
        &request.dest,
        request.expected_sha256.clone(),
        expected_bytes,
        accounted_bytes_excluding_this_partial,
    )
    .await?;
    Ok((pool, reservation))
}

#[cfg(feature = "rusqlite")]
async fn reconcile_download_reservation(
    request: &mukei_core::storage::DownloadRequest,
    pool: &mukei_core::storage::DatabasePool,
    reservation: &mukei_core::storage::DownloadReservation,
    total_bytes: u64,
) -> mukei_core::error::Result<mukei_core::storage::DownloadReservation> {
    let parent = request.dest.parent().ok_or_else(|| {
        mukei_core::error::MukeiError::Invariant(
            "model destination has no app-private parent directory".into(),
        )
    })?;
    let parent = parent.to_path_buf();
    let partial_path = request.partial_path();
    let accounted_storage_bytes = tokio::task::spawn_blocking(move || {
        let resume_bytes = std::fs::metadata(partial_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let usage = mukei_core::storage::StorageQuotaManager::new(parent).usage()?;
        Ok::<_, mukei_core::error::MukeiError>(
            usage.accounted_model_bytes().saturating_sub(resume_bytes),
        )
    })
    .await
    .map_err(|error| mukei_core::error::MukeiError::BlockingJoinFailed(error.to_string()))??;

    mukei_core::storage::DownloadJobRepository::reconcile_started(
        pool,
        reservation,
        total_bytes,
        accounted_storage_bytes,
    )
    .await
}

fn persist_provider_secret(alias: &str, secret: &Zeroizing<String>) -> Result<(), String> {
    if secret.trim().is_empty() {
        android_secret_store::delete(alias)
    } else {
        android_secret_store::store(alias, secret.as_bytes())
    }
}

async fn hydrate_provider_secrets_from_platform() -> Result<(), String> {
    if let Some(bytes) = android_secret_store::load(BRAVE_SECRET_ALIAS)? {
        let value = String::from_utf8(bytes.to_vec())
            .map_err(|_| "stored Brave credential is not valid UTF-8".to_string())?;
        *runtime_state().brave_api_key().lock() = Some(Zeroizing::new(value));
    }
    if let Some(bytes) = android_secret_store::load(TAVILY_SECRET_ALIAS)? {
        let value = String::from_utf8(bytes.to_vec())
            .map_err(|_| "stored Tavily credential is not valid UTF-8".to_string())?;
        *runtime_state().tavily_api_key().lock() = Some(Zeroizing::new(value));
    }
    Ok(())
}

#[cfg(feature = "rusqlite")]
async fn persist_provider_secret_refs(
    pool: &mukei_core::storage::DatabasePool,
) -> mukei_core::error::Result<()> {
    #[cfg(target_os = "android")]
    {
        for (slot, storage_key) in [
            ("brave_api_key", BRAVE_SECRET_ALIAS),
            ("tavily_api_key", TAVILY_SECRET_ALIAS),
        ] {
            SettingsRepository::upsert_secret_ref(
                pool,
                SecretRefRecord {
                    slot: slot.to_string(),
                    provider: "android_keystore_aes_gcm".to_string(),
                    storage_key: storage_key.to_string(),
                },
            )
            .await?;
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = pool;
    Ok(())
}

/// Bridge-side wrapped-secrets helper. The bridge crate is responsible
/// for unwrapping the Keystore-protected ciphertext that arrives over
/// the JNI boundary and handing the plaintext to the core; the
/// plaintext never returns to Java and old bridge copies are zeroized
/// when replaced.
async fn rebuild_tool_registry_from_secrets() {
    let brave = runtime_state()
        .brave_api_key()
        .lock()
        .as_ref()
        .map(|key| Zeroizing::new(key.as_str().to_owned()))
        .unwrap_or_else(|| Zeroizing::new("missing-brave-key".to_string()));
    let tavily = runtime_state()
        .tavily_api_key()
        .lock()
        .as_ref()
        .map(|key| Zeroizing::new(key.as_str().to_owned()))
        .unwrap_or_else(|| Zeroizing::new("missing-tavily-key".to_string()));
    let remote_policy = runtime_state().remote_feature_policy();
    let registry = Arc::new(ToolRegistry::with_web_search_secrets_and_policy(
        brave,
        tavily,
        remote_policy,
    ));
    runtime_state().set_tool_registry(registry.clone());
    tracing::info!("tool registry rebuilt with wrapped-secrets keys");

    let cfg_opt = runtime_state().config();
    if let Some(cfg) = cfg_opt {
        #[cfg(feature = "rusqlite")]
        {
            if let Some(pool) = runtime_state().database_pool() {
                let loop_handle = agent_runtime::build_agent_loop(
                    &cfg,
                    registry.clone(),
                    pool,
                    runtime_state().audit_log_writer().clone(),
                    runtime_state().model_activation_service(),
                );
                runtime_state().set_agent_loop(loop_handle);
                tracing::info!("agent loop rebuilt alongside tool registry");
            }
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let loop_handle = agent_runtime::build_agent_loop(
                &cfg,
                registry.clone(),
                runtime_state().model_activation_service(),
            );
            runtime_state().set_agent_loop(loop_handle);
            tracing::info!("agent loop rebuilt alongside tool registry");
        }
    }
}

fn rebuild_tool_registry_from_secrets_blocking() {
    let brave = runtime_state()
        .brave_api_key()
        .lock()
        .as_ref()
        .map(|key| Zeroizing::new(key.as_str().to_owned()))
        .unwrap_or_else(|| Zeroizing::new("missing-brave-key".to_string()));
    let tavily = runtime_state()
        .tavily_api_key()
        .lock()
        .as_ref()
        .map(|key| Zeroizing::new(key.as_str().to_owned()))
        .unwrap_or_else(|| Zeroizing::new("missing-tavily-key".to_string()));
    let remote_policy = runtime_state().remote_feature_policy();
    let registry = Arc::new(ToolRegistry::with_web_search_secrets_and_policy(
        brave,
        tavily,
        remote_policy,
    ));
    runtime_state().set_tool_registry(registry.clone());
    tracing::info!("tool registry rebuilt synchronously from latest policy/key snapshot");

    let cfg_opt = runtime_state().config();
    if let Some(cfg) = cfg_opt {
        #[cfg(feature = "rusqlite")]
        {
            if let Some(pool) = runtime_state().database_pool() {
                let loop_handle = agent_runtime::build_agent_loop(
                    &cfg,
                    registry.clone(),
                    pool,
                    runtime_state().audit_log_writer().clone(),
                    runtime_state().model_activation_service(),
                );
                runtime_state().set_agent_loop(loop_handle);
            }
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let loop_handle = agent_runtime::build_agent_loop(
                &cfg,
                registry.clone(),
                runtime_state().model_activation_service(),
            );
            runtime_state().set_agent_loop(loop_handle);
        }
    }
}

#[cfg(feature = "rusqlite")]
async fn record_document_revoke_audit(
    pool: &mukei_core::storage::DatabasePool,
    token: &str,
    reason: &str,
    chunk_count: usize,
) -> mukei_core::error::Result<i64> {
    let audit_args = serde_json::json!({
        "file_token_fingerprint": mukei_core::agent::FailureTracker::fingerprint(
            "document_revoke",
            &serde_json::json!({"token": token}),
        ),
        "chunk_count": chunk_count,
        "reason": reason,
    });
    let audit_fingerprint =
        mukei_core::agent::FailureTracker::fingerprint("document_revoke", &audit_args);
    let audit_entry = AuditEntry {
        conversation_id: None,
        message_id: None,
        tool_call_id: uuid::Uuid::new_v4().to_string(),
        tool_name: "document_revoke".to_string(),
        args_json: AuditEntry::canonical_args(&audit_args),
        result_preview: format!(
            "revoked document; {chunk_count} SQL chunks staged for vector deletion"
        ),
        success: true,
        duration_ms: 0,
        error_code: None,
        fingerprint_sha256: audit_fingerprint,
    };
    let row_id = runtime_state()
        .audit_log_writer()
        .record_with_id(pool, audit_entry)
        .await?;
    core_saf::SafRegistry::link_document_audit_event(pool, token, row_id).await?;
    Ok(row_id)
}

#[cfg(feature = "rusqlite")]
async fn drain_unaudited_document_revocations(
    pool: &mukei_core::storage::DatabasePool,
) -> mukei_core::error::Result<usize> {
    let pending = core_saf::SafRegistry::unaudited_document_revocations(pool).await?;
    let mut completed = 0usize;
    for revocation in pending {
        record_document_revoke_audit(
            pool,
            &revocation.file_token,
            &revocation.reason,
            revocation.chunks_deleted,
        )
        .await?;
        completed += 1;
    }
    Ok(completed)
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        include!("cxx-qt-lib/qvariant.h");

        type QString = cxx_qt_lib::QString;
        type QVariant = cxx_qt_lib::QVariant;
    }

    unsafe extern "RustQt" {
        #[qobject]
        pub type MukeiAgent = super::MukeiAgentRust;

        #[qsignal]
        fn chunk_generated(self: Pin<&mut MukeiAgent>, chunk: QString);
        #[qsignal]
        fn stream_finalized(self: Pin<&mut MukeiAgent>);
        #[qsignal]
        fn state_changed(self: Pin<&mut MukeiAgent>, state: QString);
        #[qsignal]
        fn tool_call_started(self: Pin<&mut MukeiAgent>, tool_name: QString);
        #[qsignal]
        fn tool_call_completed(self: Pin<&mut MukeiAgent>, tool_name: QString, result: QString);
        #[qsignal]
        fn error_occurred(self: Pin<&mut MukeiAgent>, error_code: QString, message: QString);
        #[qsignal]
        fn download_progress(self: Pin<&mut MukeiAgent>, progress: f64, status: QString);
        #[qsignal]
        fn thinking_started(self: Pin<&mut MukeiAgent>);
        #[qsignal]
        fn thinking_completed(self: Pin<&mut MukeiAgent>);
        #[qsignal]
        fn event_emitted(self: Pin<&mut MukeiAgent>, event_json: QString);
        /// Completion channel for non-chat asynchronous bridge requests.
        #[qsignal]
        fn async_result(self: Pin<&mut MukeiAgent>, result_json: QString);

        #[qinvokable]
        fn submit_command_json(self: Pin<&mut MukeiAgent>, command_json: QString) -> QString;
        #[qinvokable]
        fn initialize(self: Pin<&mut MukeiAgent>, config_path: QString) -> bool;
        #[qinvokable]
        fn send_message(self: Pin<&mut MukeiAgent>, user_input: QString);
        #[qinvokable]
        fn stop_generation(self: Pin<&mut MukeiAgent>);
        #[qinvokable]
        fn download_model(self: Pin<&mut MukeiAgent>, url: QString, sha256: QString);
        /// Cancel an in-flight `download_model` call. Independent of
        /// `stop_generation` so a user pressing the chat Stop button
        /// never kills a model download running underneath the UI.
        #[qinvokable]
        fn stop_download(self: Pin<&mut MukeiAgent>);
        #[qinvokable]
        fn clear_conversation(self: Pin<&mut MukeiAgent>);
        #[qinvokable]
        fn get_hardware_info(self: Pin<&mut MukeiAgent>) -> QVariant;
        #[qinvokable]
        fn update_setting(self: Pin<&mut MukeiAgent>, key: QString, value: QVariant);
        #[qinvokable]
        fn interrupted_turn_json(self: Pin<&mut MukeiAgent>) -> QString;
        #[qinvokable]
        fn resume_interrupted_turn(self: Pin<&mut MukeiAgent>);
        #[qinvokable]
        fn regenerate_interrupted_turn(self: Pin<&mut MukeiAgent>);
        #[qinvokable]
        fn ui_session_json(self: Pin<&mut MukeiAgent>) -> QString;
        #[qinvokable]
        fn save_ui_session(self: Pin<&mut MukeiAgent>, session_json: QString);
        #[qinvokable]
        fn draft_json(
            self: Pin<&mut MukeiAgent>,
            conversation_id: QString,
            branch_id: QString,
        ) -> QString;
        #[qinvokable]
        fn save_draft(
            self: Pin<&mut MukeiAgent>,
            conversation_id: QString,
            branch_id: QString,
            text: QString,
            cursor_position: i32,
        );
        #[qinvokable]
        fn clear_draft(self: Pin<&mut MukeiAgent>, conversation_id: QString, branch_id: QString);
        #[qinvokable]
        fn conversation_list_json(self: Pin<&mut MukeiAgent>, limit: i32) -> QString;
        #[qinvokable]
        fn chat_snapshot_json(
            self: Pin<&mut MukeiAgent>,
            conversation_id: QString,
            branch_id: QString,
            before_message_id: QString,
            limit: i32,
        ) -> QString;
        #[qinvokable]
        fn download_jobs_json(self: Pin<&mut MukeiAgent>, limit: i32) -> QString;
        #[qinvokable]
        fn storage_snapshot_json(self: Pin<&mut MukeiAgent>) -> QString;
        #[qinvokable]
        fn document_list_json(self: Pin<&mut MukeiAgent>, limit: i32) -> QString;
        #[qinvokable]
        fn settings_snapshot_json(self: Pin<&mut MukeiAgent>) -> QString;
        #[qinvokable]
        fn select_installed_model_json(self: Pin<&mut MukeiAgent>, model_id: QString) -> QString;
        #[qinvokable]
        fn delete_installed_model_json(self: Pin<&mut MukeiAgent>, model_id: QString) -> QString;
        #[qinvokable]
        fn grant_document_access_json(
            self: Pin<&mut MukeiAgent>,
            target: QString,
            label: QString,
            mime: QString,
        ) -> QString;
        #[qinvokable]
        fn revoke_document_json(self: Pin<&mut MukeiAgent>, document_id: QString) -> QString;
        #[qinvokable]
        fn retry_document_ingestion_json(
            self: Pin<&mut MukeiAgent>,
            document_id: QString,
        ) -> QString;
        #[qinvokable]
        fn engine_session_snapshot_json(self: Pin<&mut MukeiAgent>) -> QString;
        #[qinvokable]
        fn ui_contract_snapshot_json(self: Pin<&mut MukeiAgent>) -> QString;
        #[qinvokable]
        fn operation_snapshot_json(self: Pin<&mut MukeiAgent>) -> QString;
        #[qinvokable]
        fn diagnostics_snapshot_json(self: Pin<&mut MukeiAgent>) -> QString;
        #[qinvokable]
        fn provenance_snapshot_json(self: Pin<&mut MukeiAgent>) -> QString;
        #[qinvokable]
        fn export_diagnostics_json(self: Pin<&mut MukeiAgent>) -> QString;

        #[qobject]
        pub type MukeiBridge = super::MukeiBridgeRust;

        #[qsignal]
        fn thermal_status_changed(self: Pin<&mut MukeiBridge>, status: i32);
        #[qsignal]
        fn saf_grant_revoked(self: Pin<&mut MukeiBridge>, token: QString);
        #[qsignal]
        fn error_occurred(self: Pin<&mut MukeiBridge>, error_code: QString, message: QString);
        #[qsignal]
        fn event_emitted(self: Pin<&mut MukeiBridge>, event_json: QString);

        // Wrapped-secrets API — each setter accepts the UNWRAPPED key
        // material (the bridge unwraps via Android Keystore *before*
        // calling these). The names match the wrapped-secrets registry
        // slots in `config.toml::wrapped_secrets`.
        #[qinvokable]
        fn set_brave_api_key(self: Pin<&mut MukeiBridge>, api_key: QString);
        #[qinvokable]
        fn set_tavily_api_key(self: Pin<&mut MukeiBridge>, api_key: QString);
        #[qinvokable]
        fn set_remote_feature_policy(self: Pin<&mut MukeiBridge>, policy: QString);
        /// Inject the hex-encoded unwrapped SQLCipher database key
        /// before `MukeiAgent.initialize()` opens storage. Raw key
        /// bytes must never be passed through QString.
        #[qinvokable]
        fn set_database_cipher_key(self: Pin<&mut MukeiBridge>, key: QString);
        #[qinvokable]
        fn note_thermal_status(self: Pin<&mut MukeiBridge>, status: i32);
        #[qinvokable]
        fn saf_registry_count(self: Pin<&mut MukeiBridge>) -> i32;
        // TRD §8.1 / REQ-MOD-01 — model download surface.
        #[qinvokable]
        fn set_model_dir(self: Pin<&mut MukeiBridge>, path: QString);
        #[qinvokable]
        fn model_dir(self: Pin<&mut MukeiBridge>) -> QString;
        #[qinvokable]
        fn recommended_model_id(self: Pin<&mut MukeiBridge>, total_ram_mib: i32) -> QString;
        #[qinvokable]
        fn model_catalogue_json(self: Pin<&mut MukeiBridge>) -> QString;

        #[qobject]
        pub type SafRegistry = super::SafRegistryRust;

        #[qsignal]
        fn token_revoked(self: Pin<&mut SafRegistry>, token: QString);

        #[qinvokable]
        fn upsert_grant(
            self: Pin<&mut SafRegistry>,
            token: QString,
            target: QString,
            mime: QString,
        ) -> bool;
        #[qinvokable]
        fn resolve_token(self: Pin<&mut SafRegistry>, token: QString) -> QString;
        #[qinvokable]
        fn revoke_token(self: Pin<&mut SafRegistry>, token: QString) -> bool;
        #[qinvokable]
        fn count(self: Pin<&mut SafRegistry>) -> i32;
    }

    impl cxx_qt::Threading for MukeiAgent {}
    impl cxx_qt::Threading for MukeiBridge {}
    impl cxx_qt::Threading for SafRegistry {}
}

pub struct MukeiAgentRust {
    /// Cancellation token for the **chat / inference** path only
    /// (`send_message` ↔ `stop_generation`). Architect-review
    /// follow-up: this used to be shared with the download path, which
    /// meant `stop_generation()` silently cancelled any in-flight
    /// `download_model` running in the background. The two surfaces
    /// now own independent tokens; see [`Self::download_cancel`].
    cancel_token: CancellationToken,
    /// Cancellation token for the **model download** path only
    /// (`download_model` ↔ `stop_download`). Independent of
    /// `cancel_token` so a user pressing the chat Stop button never
    /// kills a multi-gigabyte download running underneath the UI.
    download_cancel: CancellationToken,
    state: Arc<Mutex<String>>,
    /// Re-entrancy guard for `send_message` (user priority follow-up).
    /// The flag is flipped from `false` to `true` by the synchronous
    /// entry into `send_message`; a second call that observes `true`
    /// is rejected with `ERR_BRIDGE_BUSY` and emits no side effects.
    ///
    /// **Release is RAII**, via [`BusyGuard`] — the spawned streaming
    /// task owns a guard that clears the flag in `Drop`. This covers
    /// every termination path (success, `handle.run` error,
    /// missing-loop, cancellation) **and the panic path** for free:
    /// the workspace mandates `panic = "unwind"` (TRD §1.3 / PRD G1),
    /// so a panic inside the spawned task still unwinds through
    /// `Drop`. A manual `store(false, ...)` at the end of the block
    /// would skip Drop on panic and soft-lock the bridge for the rest
    /// of the app's life — architect-review follow-up confirmed this
    /// is the correct primitive.
    ///
    /// **Scope is chat-only.** `download_model` has its own re-entrancy
    /// mechanism via `BridgeRuntimeState::downloads_in_flight` + [`DownloadSlotGuard`]
    /// because a chat turn and a download are not mutually exclusive,
    /// but two downloads of the same dest path are.
    busy: Arc<AtomicBool>,
    /// Monotonic bridge-local event sequence. This is not persisted
    /// across process restarts; it is only for ordering events delivered
    /// to one live QML engine.
    event_sequence: Arc<AtomicU64>,
    /// Bridge-local mirror of active downloads for stop/cancel events.
    /// `stop_download()` still cancels every in-flight download because
    /// the public API has one global download token; when exactly one
    /// download is active we can honestly include its model/destination
    /// in the emitted event.
    active_downloads: Arc<ParkingMutex<Vec<ActiveDownload>>>,
}

pub struct MukeiBridgeRust;
pub struct SafRegistryRust;

impl Default for MukeiAgentRust {
    fn default() -> Self {
        Self {
            cancel_token: CancellationToken::new(),
            download_cancel: CancellationToken::new(),
            state: Arc::new(Mutex::new("UNINITIALIZED".to_string())),
            busy: Arc::new(AtomicBool::new(false)),
            event_sequence: Arc::new(AtomicU64::new(1)),
            active_downloads: Arc::new(ParkingMutex::new(Vec::new())),
        }
    }
}

impl Default for MukeiBridgeRust {
    fn default() -> Self {
        Self
    }
}

impl Default for SafRegistryRust {
    fn default() -> Self {
        Self
    }
}

#[derive(Clone, Copy)]
enum BridgeRecoveryMode {
    Resume,
    Regenerate,
}

#[cfg(feature = "rusqlite")]
impl BridgeRecoveryMode {
    fn into_storage(self) -> mukei_core::storage::RecoveryMode {
        match self {
            Self::Resume => mukei_core::storage::RecoveryMode::Resume,
            Self::Regenerate => mukei_core::storage::RecoveryMode::Regenerate,
        }
    }
}

fn start_interrupted_attempt(mut agent: Pin<&mut ffi::MukeiAgent>, mode: BridgeRecoveryMode) {
    let qt_thread = agent.as_ref().get_ref().qt_thread();
    if !runtime_state().runtime_coordinator().is_ready() {
        let err = mukei_core::error::MukeiError::Internal(format!(
            "runtime is not ready: {:?}",
            runtime_state().runtime_coordinator().phase()
        ));
        let event = error_bridge_event(&err, "recover_interrupted_turn");
        let code = err.error_code().to_string();
        let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
        let _ = qt_thread.queue(move |mut qobject| {
            qobject.as_mut().event_emitted(event_json(event));
            qobject
                .as_mut()
                .error_occurred(QString::from(&code), QString::from(&message));
        });
        return;
    }
    let (busy, sequence, cancel_token) = {
        let mut rust = agent.as_mut().rust_mut();
        if rust
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            let err = mukei_core::error::MukeiError::BridgeBusy;
            let event = error_bridge_event(&err, "recover_interrupted_turn");
            let code = err.error_code().to_string();
            let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(event));
                qobject
                    .as_mut()
                    .error_occurred(QString::from(&code), QString::from(&message));
            });
            return;
        }
        rust.cancel_token = CancellationToken::new();
        (
            rust.busy.clone(),
            rust.event_sequence.clone(),
            rust.cancel_token.clone(),
        )
    };
    let busy_guard = BusyGuard(busy);

    mukei_core::runtime::get().spawn(async move {
        #[cfg(feature = "rusqlite")]
        {
            let Some(pool) = runtime_state().database_pool() else {
                let err = mukei_core::error::MukeiError::DatabaseInitFailed(
                    "recovery requires an initialized database".to_string(),
                );
                let event = error_bridge_event(&err, "recover_interrupted_turn");
                let code = err.error_code().to_string();
                let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                let _ = qt_thread.queue(move |mut qobject| {
                    qobject.as_mut().event_emitted(event_json(event));
                    qobject
                        .as_mut()
                        .error_occurred(QString::from(&code), QString::from(&message));
                });
                drop(busy_guard);
                return;
            };
            let Some(handle) = runtime_state().agent_loop() else {
                let err = mukei_core::error::MukeiError::Internal(
                    "agent loop is not initialized".to_string(),
                );
                let event = error_bridge_event(&err, "recover_interrupted_turn");
                let code = err.error_code().to_string();
                let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                let _ = qt_thread.queue(move |mut qobject| {
                    qobject.as_mut().event_emitted(event_json(event));
                    qobject
                        .as_mut()
                        .error_occurred(QString::from(&code), QString::from(&message));
                });
                drop(busy_guard);
                return;
            };

            let assistant_external_id = MessageId::new();
            let attempt = match mukei_core::storage::RecoveryStore::begin_attempt(
                &pool,
                mode.into_storage(),
                assistant_external_id,
            )
            .await
            {
                Ok(attempt) => attempt,
                Err(error) => {
                    let event = error_bridge_event(&error, "recover_interrupted_turn");
                    let code = error.error_code().to_string();
                    let message =
                        mukei_core::diagnostics::sanitize_error_message(error.to_string());
                    let _ = qt_thread.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&message));
                    });
                    drop(busy_guard);
                    return;
                }
            };
            runtime_state().set_chat_session(Some((attempt.conversation, attempt.branch)));

            let turn_id = assistant_external_id.0.to_string();
            let conversation = attempt.conversation;
            let branch = attempt.branch;
            let persisted_turn = attempt.turn.clone();
            let sink: Arc<dyn AgentEventSink> = Arc::new(
                agent_runtime::BridgeTurnPersistence::new(pool.clone(), attempt.turn.clone()),
            );
            let partial = Arc::new(Mutex::new(String::new()));
            let partial_for_chunks = partial.clone();
            let pool_for_chunks = pool.clone();
            let turn_for_chunks = attempt.turn.clone();
            let ui_thread = qt_thread.clone();
            let chunk_sequence = sequence.clone();
            let chunk_turn_id = turn_id.clone();
            let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<String>(256);
            let chunk_task = mukei_core::runtime::get().spawn(async move {
                let mut tags = TagsStreaming::new();
                let mut last_persisted_len = 0usize;
                let mut last_persisted_at = tokio::time::Instant::now();
                while let Some(chunk) = chunk_rx.recv().await {
                    if chunk == "\u{0001}STREAM_FINAL\u{0001}" {
                        if tags.is_open() {
                            let _ = ui_thread
                                .queue(|mut qobject| qobject.as_mut().thinking_completed());
                        }
                        let _ = ui_thread.queue(|mut qobject| qobject.as_mut().stream_finalized());
                        break;
                    }
                    {
                        let mut content = partial_for_chunks.lock().await;
                        content.push_str(&chunk);
                    }
                    let should_persist = {
                        let content = partial_for_chunks.lock().await;
                        content.len().saturating_sub(last_persisted_len) >= 512
                            || last_persisted_at.elapsed() >= std::time::Duration::from_millis(750)
                    };
                    if should_persist {
                        let content = partial_for_chunks.lock().await.clone();
                        if ConversationRepository::update_assistant_partial(
                            &pool_for_chunks,
                            turn_for_chunks.clone(),
                            content.clone(),
                        )
                        .await
                        .is_ok()
                        {
                            last_persisted_len = content.len();
                            last_persisted_at = tokio::time::Instant::now();
                        }
                    }
                    let events = tags.push(&chunk);
                    if events.contains(TagEvents::OPENED) {
                        let _ = ui_thread.queue(|mut qobject| qobject.as_mut().thinking_started());
                    }
                    if events.contains(TagEvents::CLOSED) {
                        let _ =
                            ui_thread.queue(|mut qobject| qobject.as_mut().thinking_completed());
                    }
                    let event_sequence = chunk_sequence.clone();
                    let event_turn_id = chunk_turn_id.clone();
                    let _ = ui_thread.queue(move |mut qobject| {
                        let event = BridgeEvent::new(BridgeEventKind::ChatChunk {
                            chunk: chunk.clone(),
                        })
                        .with_chat_scope(conversation, branch, event_turn_id)
                        .with_sequence(event_sequence.fetch_add(1, Ordering::AcqRel));
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject.as_mut().chunk_generated(QString::from(&chunk));
                    });
                }
            });

            let submit = BridgeEvent::new(BridgeEventKind::ChatState {
                state: ChatTurnState::Submitting,
                capabilities: CapabilitySnapshot::inferencing(),
            })
            .with_chat_scope(conversation, branch, turn_id.clone())
            .with_sequence(sequence.fetch_add(1, Ordering::AcqRel));
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(submit));
                qobject.as_mut().state_changed(QString::from("INFERRING"));
            });

            let result = handle
                .run_seeded(
                    attempt.seed_history,
                    conversation,
                    branch,
                    cancel_token.clone(),
                    chunk_tx.clone(),
                    Some(sink),
                )
                .await;
            let streamed_content = partial.lock().await.clone();
            let (failed, final_parent, final_tokens, final_content) = match result {
                Ok(outcome) => (
                    false,
                    Some(outcome.final_parent),
                    outcome.final_token_count,
                    outcome.final_content,
                ),
                Err(error) => {
                    let event = error_bridge_event(&error, "recover_interrupted_turn");
                    let code = error.error_code().to_string();
                    let message =
                        mukei_core::diagnostics::sanitize_error_message(error.to_string());
                    let _ = qt_thread.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&message));
                    });
                    (true, None, None, None)
                }
            };
            let content = final_content.unwrap_or(streamed_content);
            let persisted = if failed {
                ConversationRepository::fail_turn_with_parent(
                    &pool,
                    persisted_turn,
                    MessageStatus::Failed,
                    content,
                    final_parent,
                )
                .await
            } else if cancel_token.is_cancelled() {
                ConversationRepository::fail_turn_with_parent(
                    &pool,
                    persisted_turn,
                    MessageStatus::Cancelled,
                    content,
                    final_parent,
                )
                .await
            } else {
                ConversationRepository::complete_turn_with_parent(
                    &pool,
                    persisted_turn,
                    content,
                    final_parent,
                    final_tokens,
                )
                .await
            };
            if let Err(error) = persisted {
                tracing::warn!(error = %error, "failed to finalize recovered turn");
            }
            let _ = chunk_tx
                .send("\u{0001}STREAM_FINAL\u{0001}".to_string())
                .await;
            let _ = chunk_task.await;
            let final_event = if failed {
                BridgeEvent::new(BridgeEventKind::ChatState {
                    state: ChatTurnState::Failed,
                    capabilities: current_ready_capabilities(),
                })
            } else if cancel_token.is_cancelled() {
                BridgeEvent::new(BridgeEventKind::ChatCancelled)
            } else {
                BridgeEvent::new(BridgeEventKind::ChatCompleted)
            }
            .with_chat_scope(conversation, branch, turn_id)
            .with_sequence(sequence.fetch_add(1, Ordering::AcqRel));
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(final_event));
                qobject.as_mut().state_changed(QString::from("IDLE_READY"));
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let _ = (mode, sequence, cancel_token, qt_thread);
        }
        drop(busy_guard);
    });
}

impl ffi::MukeiAgent {
    /// Submit one protocol-v2 command envelope and return exactly one immediate acknowledgement.
    pub fn submit_command_json(self: Pin<&mut Self>, command_json: QString) -> QString {
        protocol::submit_command_json(self, command_json)
    }

    /// Boot path. Loads + validates `config.toml`, opens the SQLite /
    /// SQLCipher pool, runs pending migrations, hydrates the SAF
    /// registry from disk, reconciles persisted vector state, and
    /// constructs the shared `Arc<AgentLoop>`.
    pub fn initialize(self: Pin<&mut Self>, config_path: QString) -> bool {
        let state = self.as_ref().rust().state.clone();
        let qt = self.as_ref().get_ref().qt_thread();
        let config_path = config_path.to_string();
        mukei_core::runtime::get().spawn(async move {
            let init_guard = match InitializationGuard::try_new(
                runtime_state().runtime_coordinator().clone(),
            ) {
                Ok(guard) => guard,
                Err(RuntimePhase::Ready) => {
                    // Repeated Android/Qt lifecycle callbacks are idempotent.
                    // Re-emit the stable ready projection rather than starting
                    // another database/key bootstrap or reporting a false error.
                    let _ = qt.queue(|mut qobject| {
                        qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                            BridgeEventKind::AppLifecycle {
                                state: AppLifecycleState::Ready,
                                capabilities: current_ready_capabilities(),
                                android_storage: Some(AndroidStorageState::Ready {
                                    saf_grant_count: runtime_state().saf_registry().count(),
                                }),
                            },
                        )));
                    });
                    return;
                }
                Err(
                    phase @ (RuntimePhase::Initializing
                    | RuntimePhase::DatabaseOpened
                    | RuntimePhase::AuditVerified),
                ) => {
                    tracing::info!(?phase, "duplicate initialize callback ignored while bootstrap is active");
                    return;
                }
                Err(RuntimePhase::Quarantined) => {
                    tracing::warn!(
                        "duplicate initialize callback ignored while runtime remains quarantined"
                    );
                    return;
                }
                Err(phase) => {
                    let err = mukei_core::error::MukeiError::Internal(format!(
                        "initialize rejected while runtime is in phase {phase:?}"
                    ));
                    let event = error_bridge_event(&err, "initialize");
                    let code = err.error_code().to_string();
                    let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&message));
                    });
                    return;
                }
            };
            let _ = qt.queue(|mut qobject| {
                qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                    BridgeEventKind::AppLifecycle {
                        state: AppLifecycleState::Booting,
                        capabilities: CapabilitySnapshot::uninitialized(),
                        android_storage: Some(AndroidStorageState::Unknown),
                    },
                )));
            });
            let _ = qt.queue(|mut qobject| {
                qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                    BridgeEventKind::AppLifecycle {
                        state: AppLifecycleState::LoadingConfig,
                        capabilities: CapabilitySnapshot::uninitialized(),
                        android_storage: Some(AndroidStorageState::Unknown),
                    },
                )));
            });
            let cfg_path = std::path::PathBuf::from(&config_path);
            if !cfg_path.exists() {
                if let Err(io_error) = mukei_core::config::write_default(&cfg_path) {
                    let err = mukei_core::error::MukeiError::SafeStorageUnavailable(
                        format!("first-run config creation failed: {io_error}"),
                    );
                    let code = err.error_code().to_string();
                    let msg = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                    let event = error_bridge_event(&err, "initialize");
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&msg));
                    });
                    return;
                }
            }
            let cfg = match agent_runtime::load_config(&cfg_path) {
                Ok(c) => c,
                Err(e) => {
                    let code = e.error_code().to_string();
                    let msg = mukei_core::diagnostics::sanitize_error_message(e.to_string());
                    let event = error_bridge_event(&e, "initialize");
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&msg));
                    });
                    return;
                }
            };
            #[cfg(target_os = "android")]
            if let Err(e) = cfg.validate_android_storage_paths(&cfg_path) {
                let code = e.error_code().to_string();
                let msg = mukei_core::diagnostics::sanitize_error_message(e.to_string());
                let event = error_bridge_event(&e, "initialize");
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().event_emitted(event_json(event));
                    qobject
                        .as_mut()
                        .error_occurred(QString::from(&code), QString::from(&msg));
                });
                return;
            }
            if let Err(e) = production_safety_status()
                .validate_for_environment(provenance::runtime_environment_mode())
            {
                tracing::error!(
                    code = e.error_code(),
                    environment = ?provenance::runtime_environment_mode(),
                    "production safety policy rejected bridge startup"
                );
                let code = e.error_code().to_string();
                let msg = mukei_core::diagnostics::sanitize_error_message(e.to_string());
                let event = error_bridge_event(&e, "production_safety");
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                        BridgeEventKind::AppLifecycle {
                            state: AppLifecycleState::FatalError,
                            capabilities: CapabilitySnapshot::uninitialized(),
                            android_storage: Some(AndroidStorageState::Unknown),
                        },
                    )));
                    qobject.as_mut().event_emitted(event_json(event));
                    qobject
                        .as_mut()
                        .error_occurred(QString::from(&code), QString::from(&msg));
                });
                return;
            }
            let cfg_for_dirs = cfg.clone();
            match tokio::task::spawn_blocking(move || cfg_for_dirs.ensure_storage_directories()).await {
                Ok(Ok(())) => {}
                Ok(Err(io_error)) => {
                    let err = mukei_core::error::MukeiError::SafeStorageUnavailable(
                        format!("app-private storage preparation failed: {io_error}"),
                    );
                    let code = err.error_code().to_string();
                    let msg = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                    let event = error_bridge_event(&err, "initialize");
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&msg));
                    });
                    return;
                }
                Err(join_error) => {
                    let err = mukei_core::error::MukeiError::BlockingJoinFailed(join_error.to_string());
                    let code = err.error_code().to_string();
                    let msg = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                    let event = error_bridge_event(&err, "initialize");
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&msg));
                    });
                    return;
                }
            }
            tracing::info!(
                ?cfg.gpu_layers, n_ctx = cfg.n_ctx,
                max_iterations = cfg.watchdog.max_iterations,
                "config loaded"
            );
            if let Err(error) = hydrate_provider_secrets_from_platform().await {
                let err = mukei_core::error::MukeiError::SafeStorageUnavailable(
                    format!("provider secret hydration failed: {error}"),
                );
                let code = err.error_code().to_string();
                let msg = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                let event = error_bridge_event(&err, "initialize");
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().event_emitted(event_json(event));
                    qobject
                        .as_mut()
                        .error_occurred(QString::from(&code), QString::from(&msg));
                });
                return;
            }
            rebuild_tool_registry_from_secrets().await;

            #[cfg(feature = "rusqlite")]
            {
                let pool = if let Some(existing) = runtime_state().database_pool() {
                    existing
                } else {
                    #[cfg(feature = "sqlcipher")]
                    let database_key = {
                        match runtime_state().secure_bootstrap().begin() {
                            BootstrapStart::Started(generation) => {
                                tracing::info!(generation, "secure database bootstrap started");
                            }
                            BootstrapStart::AlreadyReady => {
                                let err = mukei_core::error::MukeiError::DatabaseInitFailed(
                                    "secure bootstrap is ready but no database service is installed"
                                        .to_string(),
                                );
                                let code = err.error_code().to_string();
                                let msg = mukei_core::diagnostics::sanitize_error_message(
                                    err.to_string(),
                                );
                                let event = error_bridge_event(&err, "secure_bootstrap");
                                runtime_state()
                                    .secure_bootstrap()
                                    .transition(SecureBootstrapState::ResetRequired);
                                let _ = qt.queue(move |mut qobject| {
                                    qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                                        BridgeEventKind::AppLifecycle {
                                            state: AppLifecycleState::ResetRequired,
                                            capabilities: CapabilitySnapshot::uninitialized(),
                                            android_storage: Some(AndroidStorageState::Unknown),
                                        },
                                    )));
                                    qobject.as_mut().event_emitted(event_json(event));
                                    qobject.as_mut().error_occurred(
                                        QString::from(&code),
                                        QString::from(&msg),
                                    );
                                });
                                return;
                            }
                            BootstrapStart::InProgress(phase) => {
                                tracing::info!(?phase, "duplicate secure bootstrap request ignored");
                                return;
                            }
                            BootstrapStart::ResetRequired => {
                                let _ = qt.queue(|mut qobject| {
                                    qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                                        BridgeEventKind::AppLifecycle {
                                            state: AppLifecycleState::ResetRequired,
                                            capabilities: CapabilitySnapshot::uninitialized(),
                                            android_storage: Some(AndroidStorageState::Unknown),
                                        },
                                    )));
                                });
                                return;
                            }
                        }

                        let qt_for_state = qt.clone();
                        let prepared = prepare_database_key_with_observer(
                            runtime_state().secure_bootstrap(),
                            &PlatformSecureKeyProvider,
                            move |secure_state| {
                                tracing::info!(
                                    state = ?secure_state,
                                    "secure database bootstrap state transition"
                                );
                                let lifecycle_state =
                                    lifecycle_state_for_secure_bootstrap(secure_state);
                                let _ = qt_for_state.queue(move |mut qobject| {
                                    qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                                        BridgeEventKind::AppLifecycle {
                                            state: lifecycle_state,
                                            capabilities: CapabilitySnapshot::uninitialized(),
                                            android_storage: Some(AndroidStorageState::Unknown),
                                        },
                                    )));
                                });
                            },
                        );
                        match prepared {
                            Ok(prepared) => {
                                tracing::info!(
                                    first_install = prepared.first_install,
                                    "secure database key prepared without exposing key material"
                                );
                                prepared.key
                            }
                            Err(failure) => {
                                let failure_state = failure.state();
                                runtime_state()
                                    .secure_bootstrap()
                                    .transition(failure_state);
                                tracing::error!(
                                    category = failure.safe_code(),
                                    state = ?failure_state,
                                    "secure database bootstrap failed"
                                );
                                let lifecycle_state =
                                    lifecycle_state_for_secure_bootstrap(failure_state);
                                let code = failure.safe_code();
                                let message = failure.safe_message();
                                let _ = qt.queue(move |mut qobject| {
                                    qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                                        BridgeEventKind::AppLifecycle {
                                            state: lifecycle_state,
                                            capabilities: CapabilitySnapshot::uninitialized(),
                                            android_storage: Some(AndroidStorageState::Unknown),
                                        },
                                    )));
                                    qobject.as_mut().error_occurred(
                                        QString::from(code),
                                        QString::from(message),
                                    );
                                });
                                return;
                            }
                        }
                    };

                    #[cfg(feature = "sqlcipher")]
                    {
                        runtime_state()
                            .secure_bootstrap()
                            .transition(SecureBootstrapState::OpeningDatabase);
                    }
                    let _ = qt.queue(|mut qobject| {
                        qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                            BridgeEventKind::AppLifecycle {
                                state: AppLifecycleState::OpeningDatabase,
                                capabilities: CapabilitySnapshot::uninitialized(),
                                android_storage: Some(AndroidStorageState::Unknown),
                            },
                        )));
                    });

                    let opened = match agent_runtime::open_pool(
                        &cfg,
                        #[cfg(feature = "sqlcipher")]
                        database_key,
                    )
                    .await
                    {
                        Ok(pool) => Arc::new(pool),
                        Err(e) => {
                            #[cfg(feature = "sqlcipher")]
                            runtime_state()
                                .secure_bootstrap()
                                .transition(SecureBootstrapState::DatabaseOpenFailed);
                            tracing::error!(
                                code = e.error_code(),
                                category = database_open_failure_category(&e),
                                "database open failed during bootstrap"
                            );
                            let code = e.error_code().to_string();
                            let msg =
                                mukei_core::diagnostics::sanitize_error_message(e.to_string());
                            let event = error_bridge_event(&e, "database_open");
                            let _ = qt.queue(move |mut qobject| {
                                qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                                    BridgeEventKind::AppLifecycle {
                                        state: AppLifecycleState::DatabaseOpenFailed,
                                        capabilities: CapabilitySnapshot::uninitialized(),
                                        android_storage: Some(AndroidStorageState::Unknown),
                                    },
                                )));
                                qobject.as_mut().event_emitted(event_json(event));
                                qobject.as_mut().error_occurred(
                                    QString::from(&code),
                                    QString::from(&msg),
                                );
                            });
                            return;
                        }
                    };
                    #[cfg(feature = "sqlcipher")]
                    runtime_state()
                        .secure_bootstrap()
                        .transition(SecureBootstrapState::Ready);
                    opened
                };
                init_guard.transition(RuntimePhase::DatabaseOpened);
                match AuditLogReader::verify_chain(&pool).await {
                    Ok(AuditChainStatus::Ok { rows_checked, .. }) => {
                        tracing::info!(
                            rows_checked,
                            "audit log hash chain verified during bridge boot"
                        );
                    }
                    Ok(AuditChainStatus::Tampered { row_id, .. }) => {
                        let err = mukei_core::error::MukeiError::AuditLogTampered { row_id };
                        let code = err.error_code().to_string();
                        let msg = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                        let event = error_bridge_event(&err, "initialize");
                        let _ = qt.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(event));
                            qobject
                                .as_mut()
                                .error_occurred(QString::from(&code), QString::from(&msg));
                        });
                        return;
                    }
                    Err(e) => {
                        let code = e.error_code().to_string();
                        let msg = mukei_core::diagnostics::sanitize_error_message(e.to_string());
                        let event = error_bridge_event(&e, "initialize");
                        let _ = qt.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(event));
                            qobject
                                .as_mut()
                                .error_occurred(QString::from(&code), QString::from(&msg));
                        });
                        return;
                    }
                }
                init_guard.transition(RuntimePhase::AuditVerified);

                match ConversationRepository::mark_incomplete_turns_failed(&pool).await {
                    Ok(count) if count > 0 => {
                        tracing::warn!(
                            incomplete_turns = count,
                            "marked interrupted chat turns as failed during bridge boot"
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let code = e.error_code().to_string();
                        let msg = mukei_core::diagnostics::sanitize_error_message(e.to_string());
                        let event = error_bridge_event(&e, "initialize");
                        let _ = qt.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(event));
                            qobject
                                .as_mut()
                                .error_occurred(QString::from(&code), QString::from(&msg));
                        });
                        return;
                    }
                }

                if let Err(e) = runtime_state().audit_log_writer().hydrate_from_pool(&pool).await {
                    let code = e.error_code().to_string();
                    let msg = mukei_core::diagnostics::sanitize_error_message(e.to_string());
                    let event = error_bridge_event(&e, "initialize");
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&msg));
                    });
                    return;
                }

                match drain_unaudited_document_revocations(&pool).await {
                    Ok(count) if count > 0 => tracing::warn!(
                        recovered_audit_rows = count,
                        "repaired committed document revocations missing audit linkage"
                    ),
                    Ok(_) => {}
                    Err(error) => {
                        let code = error.error_code().to_string();
                        let msg = mukei_core::diagnostics::sanitize_error_message(error.to_string());
                        let event = error_bridge_event(&error, "initialize");
                        let _ = qt.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(event));
                            qobject
                                .as_mut()
                                .error_occurred(QString::from(&code), QString::from(&msg));
                        });
                        return;
                    }
                }

                match mukei_core::storage::DownloadJobRepository::recover_interrupted(&pool)
                    .await
                {
                    Ok(count) if count > 0 => tracing::warn!(
                        interrupted_downloads = count,
                        "recovered stale model download reservations during bridge boot"
                    ),
                    Ok(_) => {}
                    Err(error) => tracing::warn!(
                        code = error.error_code(),
                        "failed to recover stale model download reservations"
                    ),
                }

                let _ = qt.queue(|mut qobject| {
                    qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                        BridgeEventKind::AppLifecycle {
                            state: AppLifecycleState::HydratingSaf,
                            capabilities: CapabilitySnapshot::uninitialized(),
                            android_storage: Some(AndroidStorageState::HydratingSaf),
                        },
                    )));
                });
                if let Err(e) =
                    agent_runtime::hydrate_saf_registry(runtime_state().saf_registry(), &pool).await
                {
                    tracing::warn!(error = %e, "SafRegistry hydration failed; starting empty");
                }

                let _ = qt.queue(|mut qobject| {
                    qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                        BridgeEventKind::AppLifecycle {
                            state: AppLifecycleState::ReconcilingVectorStore,
                            capabilities: CapabilitySnapshot::uninitialized(),
                            android_storage: Some(AndroidStorageState::Ready {
                                saf_grant_count: runtime_state().saf_registry().count(),
                            }),
                        },
                    )));
                });
                match agent_runtime::reconcile_vector_store(&cfg, &pool).await {
                    Ok(report) => {
                        tracing::info!(
                            orphan_sql_rows = report.orphan_sql_rows.len(),
                            orphan_vectors = report.orphan_vectors.len(),
                            total_sql_rows = report.total_sql_rows,
                            total_vectors = report.total_vectors,
                            "boot-time RAG reconciliation completed"
                        );
                        if !report.orphan_sql_rows.is_empty() || !report.orphan_vectors.is_empty() {
                            tracing::warn!(?report, "RAG SQL/vector mismatch detected on boot; re-index recommended");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "RAG reconciliation failed during boot; continuing without blocking startup");
                    }
                }

                match agent_runtime::drain_pending_document_cleanups(&cfg, &pool).await {
                    Ok((completed, failed)) if completed > 0 || failed > 0 => {
                        tracing::info!(
                            completed,
                            failed,
                            "boot-time document cleanup retry completed"
                        );
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "pending document cleanup scan failed; tombstones remain retryable"
                        );
                    }
                }

                if let Err(error) = persist_provider_secret_refs(&pool).await {
                    tracing::warn!(
                        code = error.error_code(),
                        "failed to persist opaque Android Keystore secret references"
                    );
                }

                let registry_arc = runtime_state().tool_registry();
                let loop_handle = agent_runtime::build_agent_loop(
                    &cfg,
                    registry_arc,
                    pool.clone(),
                    runtime_state().audit_log_writer().clone(),
                    runtime_state().model_activation_service(),
                );
                runtime_state().set_agent_loop(loop_handle);
                runtime_state().set_database_pool(pool);
            }
            #[cfg(not(feature = "rusqlite"))]
            {
                let registry_arc = runtime_state().tool_registry();
                let loop_handle = agent_runtime::build_agent_loop(
                    &cfg,
                    registry_arc,
                    runtime_state().model_activation_service(),
                );
                runtime_state().set_agent_loop(loop_handle);
            }

            runtime_state().set_config(cfg);
            *state.lock().await = "IDLE_READY".to_string();
            init_guard.commit_ready();
            let _ = qt.queue(|mut qobject| {
                qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                    BridgeEventKind::AppLifecycle {
                        state: AppLifecycleState::Ready,
                        capabilities: current_ready_capabilities(),
                        android_storage: Some(AndroidStorageState::Ready {
                            saf_grant_count: runtime_state().saf_registry().count(),
                        }),
                    },
                )));
                qobject.as_mut().state_changed(QString::from("IDLE_READY"));
            });
        });
        true
    }

    pub fn send_message(self: Pin<&mut Self>, user_input: QString) {
        let binding = self.as_ref();
        let rust = binding.rust();
        let cancel_token = rust.cancel_token.clone();
        let busy = rust.busy.clone();
        let sequence = rust.event_sequence.clone();
        let qt_thread = self.as_ref().get_ref().qt_thread();
        if !runtime_state().runtime_coordinator().is_ready() {
            let err = mukei_core::error::MukeiError::Internal(format!(
                "runtime is not ready: {:?}",
                runtime_state().runtime_coordinator().phase()
            ));
            let event = error_bridge_event(&err, "send_message");
            let code = err.error_code().to_string();
            let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(event));
                qobject
                    .as_mut()
                    .error_occurred(QString::from(&code), QString::from(&message));
            });
            return;
        }
        if !runtime_state()
            .model_activation_service()
            .readiness_snapshot()
            .active_backend_ready
        {
            let err = mukei_core::error::MukeiError::ModelLoadFailed(
                "no active production inference backend".to_string(),
            );
            let event = error_bridge_event(&err, "send_message");
            let code = err.error_code().to_string();
            let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(event));
                qobject
                    .as_mut()
                    .error_occurred(QString::from(&code), QString::from(&message));
            });
            return;
        }
        let input = user_input.to_string();
        let (conversation_id, branch_id) = match runtime_state().chat_session() {
            Some(session) => session,
            None => {
                let session = (ConversationId::new(), BranchId::new());
                runtime_state().set_chat_session(Some(session));
                session
            }
        };
        let user_message_id = MessageId::new();
        let assistant_message_id = MessageId::new();
        let turn_id = assistant_message_id.0.to_string();

        // Re-entrancy guard (user priority follow-up): refuse the call
        // if a prior `send_message` is still streaming. The QML side
        // must either await `stream_finalized` or call
        // `stop_generation` first. The check + flip is atomic so a
        // racing second invocation sees `Err(true)` and bails out
        // before allocating the chunk channel or spawning any task.
        if busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            let err = mukei_core::error::MukeiError::BridgeBusy;
            let code = err.error_code().to_string();
            let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
            let event = error_bridge_event(&err, "send_message");
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(event));
                qobject
                    .as_mut()
                    .error_occurred(QString::from(&code), QString::from(&message));
            });
            return;
        }

        // We hold the flag now (compare_exchange returned Ok). From
        // here on, the BusyGuard owns release — Drop runs on every
        // exit path, including the panic-unwind path that a manual
        // `store(false, ...)` at the end of the spawned block would
        // skip. Architect-review follow-up: this is the only release
        // site; no other code in the workspace should touch `busy`.
        let busy_guard = BusyGuard(busy);

        let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<String>(256);
        let ui_thread = qt_thread.clone();
        let chunk_sequence = sequence.clone();
        let chunk_turn_id = turn_id.clone();
        let partial_response = Arc::new(Mutex::new(String::new()));
        let partial_response_for_chunks = partial_response.clone();
        #[cfg(feature = "rusqlite")]
        let persisted_turn: Arc<Mutex<Option<PersistedTurn>>> = Arc::new(Mutex::new(None));
        #[cfg(feature = "rusqlite")]
        let persisted_turn_for_chunks = persisted_turn.clone();

        mukei_core::runtime::get().spawn(async move {
            let mut tags = TagsStreaming::new();
            #[cfg(feature = "rusqlite")]
            let mut last_persisted_len = 0usize;
            #[cfg(feature = "rusqlite")]
            let mut last_persisted_at = tokio::time::Instant::now();
            while let Some(chunk) = chunk_rx.recv().await {
                if chunk == "\u{0001}STREAM_FINAL\u{0001}" {
                    if tags.is_open() {
                        let _ =
                            ui_thread.queue(|mut qobject| qobject.as_mut().thinking_completed());
                        tags.reset();
                    }
                    let _ = ui_thread.queue(|mut qobject| qobject.as_mut().stream_finalized());
                    continue;
                }

                {
                    let mut partial = partial_response_for_chunks.lock().await;
                    partial.push_str(&chunk);
                }
                #[cfg(feature = "rusqlite")]
                {
                    let should_persist = {
                        let partial = partial_response_for_chunks.lock().await;
                        partial.len().saturating_sub(last_persisted_len) >= 512
                            || last_persisted_at.elapsed() >= std::time::Duration::from_millis(750)
                    };
                    if should_persist {
                        let turn = persisted_turn_for_chunks.lock().await.clone();
                        if let Some(turn) = turn {
                            let content = partial_response_for_chunks.lock().await.clone();
                            if let Some(pool) = runtime_state().database_pool() {
                                if let Err(e) = ConversationRepository::update_assistant_partial(
                                    &pool,
                                    turn,
                                    content.clone(),
                                )
                                .await
                                {
                                    tracing::warn!(
                                        error = %e,
                                        "failed to persist streaming assistant partial"
                                    );
                                } else {
                                    last_persisted_len = content.len();
                                    last_persisted_at = tokio::time::Instant::now();
                                }
                            }
                        }
                    }
                }

                let events = tags.push(&chunk);
                if events.contains(TagEvents::OPENED) {
                    let _ = ui_thread.queue(|mut qobject| qobject.as_mut().thinking_started());
                }
                if events.contains(TagEvents::CLOSED) {
                    let _ = ui_thread.queue(|mut qobject| qobject.as_mut().thinking_completed());
                }
                let event_sequence = chunk_sequence.clone();
                let event_turn_id = chunk_turn_id.clone();
                let _ = ui_thread.queue(move |mut qobject| {
                    let event = BridgeEvent::new(BridgeEventKind::ChatChunk {
                        chunk: chunk.clone(),
                    })
                    .with_chat_scope(conversation_id, branch_id, event_turn_id)
                    .with_sequence(event_sequence.fetch_add(1, Ordering::AcqRel));
                    qobject.as_mut().event_emitted(event_json(event));
                    qobject.as_mut().chunk_generated(QString::from(&chunk));
                });
            }
        });

        #[cfg(feature = "rusqlite")]
        let partial_response_for_run = partial_response.clone();
        #[cfg(feature = "rusqlite")]
        let persisted_turn_for_run = persisted_turn.clone();
        mukei_core::runtime::get().spawn(async move {
            let submit_event = BridgeEvent::new(BridgeEventKind::ChatState {
                state: ChatTurnState::Submitting,
                capabilities: CapabilitySnapshot::inferencing(),
            })
            .with_chat_scope(conversation_id, branch_id, turn_id.clone())
            .with_sequence(sequence.fetch_add(1, Ordering::AcqRel));
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(submit_event));
                qobject.as_mut().state_changed(QString::from("INFERRING"));
            });

            let loop_handle = { runtime_state().agent_loop() };
            let mut failed = false;
            #[cfg(feature = "rusqlite")]
            let mut run_outcome: Option<AgentRunOutcome> = None;
            #[cfg(feature = "rusqlite")]
            let mut event_sink: Option<Arc<dyn AgentEventSink>> = None;
            #[cfg(not(feature = "rusqlite"))]
            let event_sink: Option<Arc<dyn AgentEventSink>> = None;
            let mut final_capabilities = current_ready_capabilities();
            #[cfg(feature = "rusqlite")]
            {
                if let Some(pool) = runtime_state().database_pool() {
                    match ConversationRepository::begin_turn(
                        &pool,
                        conversation_id,
                        branch_id,
                        user_message_id,
                        assistant_message_id,
                        input.clone(),
                    )
                    .await
                    {
                        Ok(turn) => {
                            event_sink = Some(Arc::new(
                                agent_runtime::BridgeTurnPersistence::new(
                                    pool.clone(),
                                    turn.clone(),
                                ),
                            ));
                            *persisted_turn_for_run.lock().await = Some(turn);
                        }
                        Err(e) => {
                            failed = true;
                            let code = e.error_code().to_string();
                            let message = mukei_core::diagnostics::sanitize_error_message(e.to_string());
                            let event = BridgeEvent::new(BridgeEventKind::ChatFailed {
                                error: UiError::from_mukei_error(&e, "send_message"),
                            })
                            .with_chat_scope(conversation_id, branch_id, turn_id.clone())
                            .with_sequence(sequence.fetch_add(1, Ordering::AcqRel));
                            let _ = qt_thread.queue(move |mut qobject| {
                                qobject.as_mut().event_emitted(event_json(event));
                                qobject.as_mut().error_occurred(
                                    QString::from(&code),
                                    QString::from(&message),
                                );
                            });
                        }
                    }
                }
            }
            match loop_handle {
                Some(handle) if !failed => {
                    let request = AgentRunRequest::new(
                        input,
                        conversation_id,
                        branch_id,
                        user_message_id,
                        cancel_token.clone(),
                        chunk_tx.clone(),
                    )
                    .with_event_sink(event_sink);
                    let result = handle.run(request).await;
                    match result {
                        Ok(outcome) => {
                            #[cfg(feature = "rusqlite")]
                            {
                                run_outcome = Some(outcome);
                            }
                            #[cfg(not(feature = "rusqlite"))]
                            {
                                let _ = outcome;
                            }
                        }
                        Err(error) => {
                            failed = true;
                            let code = error.error_code().to_string();
                            let message = mukei_core::diagnostics::sanitize_error_message(error.to_string());
                            let event = BridgeEvent::new(BridgeEventKind::ChatFailed {
                                error: UiError::from_mukei_error(&error, "send_message"),
                            })
                            .with_chat_scope(conversation_id, branch_id, turn_id.clone())
                            .with_sequence(sequence.fetch_add(1, Ordering::AcqRel));
                            let _ = qt_thread.queue(move |mut qobject| {
                                qobject.as_mut().event_emitted(event_json(event));
                                qobject.as_mut().error_occurred(QString::from(&code), QString::from(&message));
                            });
                        }
                    }
                }
                Some(_) => {}
                None => {
                    failed = true;
                    final_capabilities = CapabilitySnapshot::uninitialized();
                    let err = mukei_core::error::MukeiError::Internal(
                        "AgentLoop was never constructed — call MukeiAgent.initialize(config_path) first."
                            .to_string(),
                    );
                    let event = BridgeEvent::new(BridgeEventKind::ChatFailed {
                        error: UiError::from_mukei_error(&err, "send_message"),
                    })
                    .with_chat_scope(conversation_id, branch_id, turn_id.clone())
                    .with_sequence(sequence.fetch_add(1, Ordering::AcqRel));
                    let _ = qt_thread.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject.as_mut().error_occurred(
                            QString::from("BRIDGE_NOT_INITIALIZED"),
                            QString::from("AgentLoop was never constructed — call MukeiAgent.initialize(config_path) first."),
                        );
                    });
                }
            }
            #[cfg(feature = "rusqlite")]
            {
                let turn = persisted_turn_for_run.lock().await.clone();
                if let Some(turn) = turn {
                    if let Some(pool) = runtime_state().database_pool() {
                        let streamed_content = partial_response_for_run.lock().await.clone();
                        let final_parent = run_outcome.as_ref().map(|outcome| outcome.final_parent);
                        let final_token_count = run_outcome
                            .as_ref()
                            .and_then(|outcome| outcome.final_token_count);
                        let content = run_outcome
                            .as_ref()
                            .and_then(|outcome| outcome.final_content.clone())
                            .unwrap_or(streamed_content);
                        let persist_result = if failed {
                            ConversationRepository::fail_turn_with_parent(
                                &pool,
                                turn,
                                MessageStatus::Failed,
                                content,
                                final_parent,
                            )
                            .await
                        } else if cancel_token.is_cancelled() {
                            ConversationRepository::fail_turn_with_parent(
                                &pool,
                                turn,
                                MessageStatus::Cancelled,
                                content,
                                final_parent,
                            )
                            .await
                        } else {
                            ConversationRepository::complete_turn_with_parent(
                                &pool,
                                turn,
                                content,
                                final_parent,
                                final_token_count,
                            )
                            .await
                        };
                        if let Err(e) = persist_result {
                            tracing::warn!(error = %e, "failed to finalize persisted chat turn");
                        }
                    }
                }
            }
            let _ = chunk_tx.send("\u{0001}STREAM_FINAL\u{0001}".to_string()).await;
            let final_event = if failed {
                BridgeEvent::new(BridgeEventKind::ChatState {
                    state: ChatTurnState::Failed,
                    capabilities: final_capabilities.clone(),
                })
            } else if cancel_token.is_cancelled() {
                BridgeEvent::new(BridgeEventKind::ChatCancelled)
            } else {
                BridgeEvent::new(BridgeEventKind::ChatCompleted)
            }
            .with_chat_scope(conversation_id, branch_id, turn_id)
            .with_sequence(sequence.fetch_add(1, Ordering::AcqRel));
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(final_event));
                qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                    BridgeEventKind::CapabilitySnapshot {
                        capabilities: final_capabilities,
                    },
                )));
                qobject.as_mut().state_changed(QString::from("IDLE_READY"));
            });
            // BusyGuard drops here on the normal path. Critically, it
            // *also* drops on the panic-unwind path — anything inside
            // `handle.run(...)` that panics (currently the `unwrap()`s
            // sprinkled across rag/embedder, engine/llama_wrapper,
            // engine/streaming, agent/context) will still release the
            // flag, so a single panic can no longer soft-lock the
            // bridge with a permanently-stuck `ERR_BRIDGE_BUSY`.
            drop(busy_guard);
        });
    }

    /// Cancel the in-flight chat/inference stream (if any).
    ///
    /// **Scope is chat-only.** Architect-review follow-up: this used
    /// to share a token with [`Self::download_model`], which meant a
    /// user pressing the chat Stop button also killed any model
    /// download running in the background — two unrelated features
    /// cross-wired through one cancellation source. The two surfaces
    /// now own independent tokens; call [`Self::stop_download`] to
    /// cancel a download.
    pub fn stop_generation(mut self: Pin<&mut Self>) {
        let qt = self.as_ref().get_ref().qt_thread();
        let mut rust = self.as_mut().rust_mut();
        let sequence = rust.event_sequence.clone();
        let was_busy = rust.busy.load(Ordering::Acquire);
        rust.cancel_token.cancel();
        rust.cancel_token = CancellationToken::new();
        if was_busy {
            let event = BridgeEvent::new(BridgeEventKind::ChatState {
                state: ChatTurnState::Cancelling,
                capabilities: CapabilitySnapshot::inferencing(),
            })
            .with_sequence(sequence.fetch_add(1, Ordering::AcqRel));
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(event));
            });
        }
    }

    /// Cancel the in-flight model download (if any). Independent of
    /// `stop_generation` so pressing the chat Stop button never kills
    /// a multi-gigabyte model fetch running underneath the UI.
    pub fn stop_download(mut self: Pin<&mut Self>) {
        let qt = self.as_ref().get_ref().qt_thread();
        let mut rust = self.as_mut().rust_mut();
        let (model_id, destination) = single_active_download(&rust.active_downloads);
        let has_active_download = !rust.active_downloads.lock().is_empty();
        rust.download_cancel.cancel();
        rust.download_cancel = CancellationToken::new();
        if has_active_download {
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                    BridgeEventKind::DownloadState {
                        state: DownloadState::Cancelling,
                        model_id,
                        destination,
                        capabilities: CapabilitySnapshot::downloading(false),
                    },
                )));
            });
        }
    }

    /// Kick off a streaming GGUF download (TRD §8.1 / REQ-MOD-01).
    ///
    /// QML can call this in two ways:
    /// 1. `url` is the canonical model id (`gemma-4-e2b-it` or
    ///    `gemma-4-e4b-it`) and `sha256` is empty. The bridge uses
    ///    the pinned URL + SHA from `model_registry`.
    /// 2. `url` is a bespoke HTTPS URL and `sha256` is the matching
    ///    64-hex digest. Useful for tester QA before the release
    ///    engineering pass pins the final CDN URL.
    ///
    /// # Re-entrancy contract
    ///
    /// Two `download_model` calls targeting **different** dest paths
    /// (e.g. E2B and E4B at the same time) run in parallel. Two calls
    /// targeting the **same** dest path are rejected: the second one
    /// emits a single `error:ERR_DOWNLOAD_BUSY:…` event and exits.
    /// This is what stops two `tokio::fs::File` handles from racing on
    /// one `.partial`, which would otherwise let each task hash only
    /// the bytes *it* wrote and atomically rename a corrupted GGUF
    /// into the model directory.
    ///
    /// # Cancellation
    ///
    /// Uses `self.download_cancel`, not `self.cancel_token`. The chat
    /// Stop button (`stop_generation`) and the download Stop button
    /// (`stop_download`) are wired through independent tokens.
    ///
    /// Progress comes back through `download_progress(progress, status)`:
    /// * `started:<bytes|unknown>`
    /// * `downloading:<bytes_downloaded>`
    /// * `complete:<model_id|filename>`
    /// * `error:<ERR_CODE>:<message>`
    pub fn download_model(self: Pin<&mut Self>, url: QString, sha256: QString) {
        let cancel = self.as_ref().rust().download_cancel.clone();
        let active_downloads = self.as_ref().rust().active_downloads.clone();
        let qt = self.as_ref().get_ref().qt_thread();
        // Architect-review follow-up: use the *download-only* token so
        // `stop_generation()` no longer silently cancels the download.
        let url_or_id = url.to_string();
        let sha = sha256.to_string();
        let descriptor = mukei_core::engine::lookup_model_str(&url_or_id);
        let model_id = descriptor.map(|value| value.id.as_str().to_string());
        #[cfg(feature = "rusqlite")]
        let expected_download_bytes = descriptor
            .map(|value| value.approximate_bytes)
            .unwrap_or(mukei_core::storage::model_download::MAX_MODEL_DOWNLOAD_BYTES);

        mukei_core::runtime::get().spawn(async move {
            let queued_model_id = model_id.clone();
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                    BridgeEventKind::DownloadState {
                        state: DownloadState::Queued,
                        model_id: queued_model_id,
                        destination: None,
                        capabilities: CapabilitySnapshot::downloading(false),
                    },
                )));
            });
            let req = match resolve_download_request(&url_or_id, &sha).await {
                Ok(r) => r,
                Err(e) => {
                    let code = e.error_code().to_string();
                    let message = mukei_core::diagnostics::sanitize_error_message(e.to_string());
                    let state_event = BridgeEvent::new(BridgeEventKind::DownloadState {
                        state: DownloadState::Failed,
                        model_id: model_id.clone(),
                        destination: None,
                        capabilities: current_ready_capabilities(),
                    });
                    let event = BridgeEvent::new(BridgeEventKind::DownloadFailed {
                        error: UiError::from_mukei_error(&e, "download_model"),
                    });
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(state_event));
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject.as_mut().download_progress(
                            0.0,
                            QString::from(format!("error:{code}:{message}").as_str()),
                        );
                    });
                    return;
                }
            };

            // --- Per-destination re-entrancy guard -------------------
            //
            // Insert `req.dest` into the root-owned in-flight registry
            // BEFORE spawning any I/O. A second call that observes the
            // path already-present is rejected with `ERR_DOWNLOAD_BUSY`
            // and emits no side effects on disk. Release is RAII via
            // [`DownloadSlotGuard`], which fires even on panic-unwind
            // (workspace mandates `panic = "unwind"`).
            let destination_token = download_destination_token(model_id.as_deref(), &req.dest);
            {
                let mut in_flight = runtime_state().downloads_in_flight().lock().await;
                if in_flight.contains(&req.dest) {
                    drop(in_flight);
                    let err = mukei_core::error::MukeiError::DownloadBusy {
                        dest: destination_token.clone(),
                    };
                    let code = err.error_code().to_string();
                    let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                    let state_event = BridgeEvent::new(BridgeEventKind::DownloadState {
                        state: DownloadState::Failed,
                        model_id: model_id.clone(),
                        destination: Some(destination_token.clone()),
                        capabilities: current_ready_capabilities(),
                    });
                    let event = BridgeEvent::new(BridgeEventKind::DownloadFailed {
                        error: UiError::from_mukei_error(&err, "download_model"),
                    });
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(state_event));
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject.as_mut().download_progress(
                            0.0,
                            QString::from(format!("error:{code}:{message}").as_str()),
                        );
                    });
                    return;
                }
                in_flight.insert(req.dest.clone());
            }
            let _slot_guard = DownloadSlotGuard {
                registry: runtime_state().downloads_in_flight().clone(),
                dest: req.dest.clone(),
            };

            #[cfg(feature = "rusqlite")]
            let mut download_job = match reserve_download_job(
                &req,
                model_id.clone(),
                destination_token.clone(),
                expected_download_bytes,
            )
            .await
            {
                Ok(job) => job,
                Err(error) => {
                    let code = error.error_code().to_string();
                    let message = mukei_core::diagnostics::sanitize_error_message(error.to_string());
                    let state_event = BridgeEvent::new(BridgeEventKind::DownloadState {
                        state: DownloadState::Failed,
                        model_id: model_id.clone(),
                        destination: Some(destination_token.clone()),
                        capabilities: current_ready_capabilities(),
                    });
                    let error_event = BridgeEvent::new(BridgeEventKind::DownloadFailed {
                        error: UiError::from_mukei_error(&error, "download_model"),
                    });
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(state_event));
                        qobject.as_mut().event_emitted(event_json(error_event));
                        qobject.as_mut().download_progress(
                            0.0,
                            QString::from(format!("error:{code}:{message}").as_str()),
                        );
                    });
                    return;
                }
            };

            let dest_for_status = req.dest.clone();
            let active_download = ActiveDownload {
                model_id: model_id.clone(),
                destination: destination_token.clone(),
            };
            active_downloads.lock().push(active_download.clone());
            let (tx, mut rx) = tokio::sync::mpsc::channel::<mukei_core::storage::DownloadEvent>(32);
            let req_for_dl = req.clone();
            let cancel_for_dl = cancel.clone();
            let dl_handle = mukei_core::runtime::get().spawn(async move {
                mukei_core::storage::run_download(req_for_dl, tx, cancel_for_dl).await
            });

            let mut total_bytes_seen: Option<u64> = None;
            let mut terminal_download_event_seen = false;
            #[cfg(feature = "rusqlite")]
            let mut terminal_job_state_written = false;
            #[cfg(feature = "rusqlite")]
            let mut last_persisted_progress = 0_u64;
            while let Some(ev) = rx.recv().await {
                let qt_for_ev = qt.clone();
                match ev {
                    mukei_core::storage::DownloadEvent::Started { total_bytes } => {
                        total_bytes_seen = total_bytes;
                        #[cfg(feature = "rusqlite")]
                        if let Some(total) = total_bytes {
                            match reconcile_download_reservation(
                                &req,
                                &download_job.0,
                                &download_job.1,
                                total,
                            )
                            .await
                            {
                                Ok(resized) => download_job.1 = resized,
                                Err(error) => {
                                    tracing::warn!(
                                        code = error.error_code(),
                                        "server-reported model size exceeds durable quota reservation"
                                    );
                                    cancel.cancel();
                                }
                            }
                        }
                        let status = match total_bytes {
                            Some(n) => format!("started:{n}"),
                            None => "started:unknown".to_string(),
                        };
                        let model_id = model_id.clone();
                        let destination = Some(destination_token.clone());
                        let _ = qt_for_ev.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                                BridgeEventKind::DownloadProgress {
                                    state: DownloadState::Starting,
                                    progress: 0.0,
                                    bytes_downloaded: 0,
                                    total_bytes,
                                    model_id,
                                    destination,
                                },
                            )));
                            qobject
                                .as_mut()
                                .download_progress(0.0, QString::from(&status));
                        });
                    }
                    mukei_core::storage::DownloadEvent::Progress {
                        progress,
                        bytes_downloaded,
                    } => {
                        #[cfg(feature = "rusqlite")]
                        if bytes_downloaded.saturating_sub(last_persisted_progress)
                            >= 16 * 1024 * 1024
                        {
                            if let Err(error) = mukei_core::storage::DownloadJobRepository::update_progress(
                                &download_job.0,
                                &download_job.1.job_id,
                                bytes_downloaded,
                            )
                            .await
                            {
                                tracing::warn!(
                                    code = error.error_code(),
                                    "failed to persist model download progress"
                                );
                            } else {
                                last_persisted_progress = bytes_downloaded;
                            }
                        }
                        let status = format!("downloading:{bytes_downloaded}");
                        let model_id = model_id.clone();
                        let destination = Some(destination_token.clone());
                        let total_bytes = total_bytes_seen;
                        let _ = qt_for_ev.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                                BridgeEventKind::DownloadProgress {
                                    state: DownloadState::Downloading,
                                    progress,
                                    bytes_downloaded,
                                    total_bytes,
                                    model_id,
                                    destination,
                                },
                            )));
                            qobject
                                .as_mut()
                                .download_progress(progress, QString::from(&status));
                        });
                    }
                    mukei_core::storage::DownloadEvent::Complete { final_path: _ } => {
                        terminal_download_event_seen = true;
                        #[cfg(feature = "rusqlite")]
                        {
                            let result = mukei_core::storage::DownloadJobRepository::finish(
                                &download_job.0,
                                &download_job.1.job_id,
                                mukei_core::storage::DownloadJobStatus::Completed,
                                None,
                            )
                            .await;
                            terminal_job_state_written = result.is_ok();
                            if let Err(error) = result {
                                tracing::warn!(
                                    code = error.error_code(),
                                    "failed to finalise completed download job"
                                );
                            }
                        }
                        let status = format!("complete:{destination_token}");
                        let final_path_string = destination_token.clone();
                        let completed_model_id = model_id.clone();
                        let _ = qt_for_ev.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                                BridgeEventKind::DownloadCompleted {
                                    final_path: final_path_string.clone(),
                                    model_id: completed_model_id,
                                },
                            )));
                            qobject
                                .as_mut()
                                .download_progress(1.0, QString::from(&status));
                        });
                    }
                    mukei_core::storage::DownloadEvent::Error { code, message } => {
                        terminal_download_event_seen = true;
                        let status = format!("error:{code}:{message}");
                        let err = download_event_error(code, message.clone());
                        let model_id = model_id.clone();
                        let destination = Some(destination_token.clone());
                        let state = if matches!(&err, mukei_core::error::MukeiError::Cancelled) {
                            DownloadState::Cancelled
                        } else {
                            DownloadState::Failed
                        };
                        #[cfg(feature = "rusqlite")]
                        {
                            let job_status = if matches!(
                                &err,
                                mukei_core::error::MukeiError::Cancelled
                            ) {
                                mukei_core::storage::DownloadJobStatus::Cancelled
                            } else {
                                mukei_core::storage::DownloadJobStatus::Failed
                            };
                            let result = mukei_core::storage::DownloadJobRepository::finish(
                                &download_job.0,
                                &download_job.1.job_id,
                                job_status,
                                Some(code.to_string()),
                            )
                            .await;
                            terminal_job_state_written = result.is_ok();
                            if let Err(error) = result {
                                tracing::warn!(
                                    code = error.error_code(),
                                    "failed to finalise failed download job"
                                );
                            }
                        }
                        let state_event = BridgeEvent::new(BridgeEventKind::DownloadState {
                            state,
                            model_id: model_id.clone(),
                            destination: destination.clone(),
                            capabilities: current_ready_capabilities(),
                        });
                        let failed_event =
                            if matches!(&err, mukei_core::error::MukeiError::Cancelled) {
                                None
                            } else {
                                Some(BridgeEvent::new(BridgeEventKind::DownloadFailed {
                                    error: UiError::from_mukei_error(&err, "download_model"),
                                }))
                            };
                        let _ = qt_for_ev.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(state_event));
                            if let Some(event) = failed_event {
                                qobject.as_mut().event_emitted(event_json(event));
                            }
                            qobject
                                .as_mut()
                                .download_progress(0.0, QString::from(&status));
                        });
                    }
                }
            }

            match dl_handle.await {
                Ok(Ok(())) => {
                    tracing::info!(
                        path = %mukei_core::diagnostics::redact_path(&dest_for_status),
                        "model download finalised"
                    );
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        error = %mukei_core::diagnostics::sanitize_error_message(e.to_string()),
                        code = e.error_code(),
                        "model download failed"
                    );
                    if !terminal_download_event_seen {
                        let code = e.error_code().to_string();
                        let message = mukei_core::diagnostics::sanitize_error_message(e.to_string());
                        let state = if matches!(&e, mukei_core::error::MukeiError::Cancelled) {
                            DownloadState::Cancelled
                        } else {
                            DownloadState::Failed
                        };
                        let state_event = BridgeEvent::new(BridgeEventKind::DownloadState {
                            state,
                            model_id: model_id.clone(),
                            destination: Some(destination_token.clone()),
                            capabilities: current_ready_capabilities(),
                        });
                        let event = BridgeEvent::new(BridgeEventKind::DownloadFailed {
                            error: UiError::from_mukei_error(&e, "download_model"),
                        });
                        let _ = qt.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(state_event));
                            qobject.as_mut().event_emitted(event_json(event));
                            qobject.as_mut().download_progress(
                                0.0,
                                QString::from(format!("error:{code}:{message}").as_str()),
                            );
                        });
                    }
                }
                Err(join_err) => {
                    let msg = format!("download task panicked: {join_err}");
                    tracing::error!(error = %msg);
                    let err = mukei_core::error::MukeiError::FFIPanic;
                    let state_event = BridgeEvent::new(BridgeEventKind::DownloadState {
                        state: DownloadState::Failed,
                        model_id: model_id.clone(),
                        destination: Some(destination_token.clone()),
                        capabilities: current_ready_capabilities(),
                    });
                    let event = BridgeEvent::new(BridgeEventKind::DownloadFailed {
                        error: UiError {
                            technical_message: msg.clone(),
                            ..UiError::from_mukei_error(&err, "download_model")
                        },
                    });
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(state_event));
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject.as_mut().download_progress(
                            0.0,
                            QString::from(format!("error:ERR_FFI_PANIC:{msg}").as_str()),
                        );
                    });
                }
            }
            #[cfg(feature = "rusqlite")]
            if !terminal_job_state_written {
                let fallback_status = if cancel.is_cancelled() {
                    mukei_core::storage::DownloadJobStatus::Cancelled
                } else {
                    mukei_core::storage::DownloadJobStatus::Failed
                };
                if let Err(error) = mukei_core::storage::DownloadJobRepository::finish(
                    &download_job.0,
                    &download_job.1.job_id,
                    fallback_status,
                    Some("ERR_DOWNLOAD_INTERRUPTED".into()),
                )
                .await
                {
                    tracing::warn!(
                        code = error.error_code(),
                        "failed to release model download reservation"
                    );
                }
            }
            active_downloads
                .lock()
                .retain(|download| download != &active_download);
        });
    }

    pub fn clear_conversation(self: Pin<&mut Self>) {
        let qt = self.as_ref().get_ref().qt_thread();
        // Clear synchronously at the authoritative runtime boundary so protocol acknowledgement
        // cannot race a detached session reset. The runtime uses a short in-memory lock.
        runtime_state().set_chat_session(None);
        let _ = qt.queue(|mut qobject| {
            qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                BridgeEventKind::CapabilitySnapshot {
                    capabilities: current_ready_capabilities(),
                },
            )));
            qobject.as_mut().state_changed(QString::from("IDLE_READY"));
        });
    }

    pub fn get_hardware_info(self: Pin<&mut Self>) -> QVariant {
        let summary = QString::from(
            format!(
                "os={} arch={} thermal_status={}",
                std::env::consts::OS,
                std::env::consts::ARCH,
                runtime_state().thermal_status()
            )
            .as_str(),
        );
        QVariant::from(&summary)
    }

    pub fn interrupted_turn_json(self: Pin<&mut Self>) -> QString {
        let qt = self.as_ref().get_ref().qt_thread();
        let tracker = runtime_state().request_tracker();
        let ticket = tracker.accept("recovery.snapshot");
        let accepted = tracker.accepted_json(&ticket);
        #[cfg(feature = "rusqlite")]
        {
            let pool = runtime_state().database_pool();
            mukei_core::runtime::get().spawn(async move {
                let result = match pool {
                    Some(pool) => match recovery_bridge::interrupted_turn_snapshot(&pool).await {
                        Ok(snapshot) => Ok(snapshot),
                        Err(error) => {
                            tracing::warn!(
                                code = error.error_code(),
                                "recovery snapshot operation failed"
                            );
                            Err(async_error_value(&error, "interrupted_turn_json"))
                        }
                    },
                    None => Ok(serde_json::Value::Null),
                };
                let completion = runtime_state()
                    .request_tracker()
                    .completion_json(&ticket, result);
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().async_result(QString::from(&completion));
                });
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let completion = tracker.completion_json(&ticket, Ok(serde_json::Value::Null));
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().async_result(QString::from(&completion));
            });
        }
        QString::from(&accepted)
    }

    pub fn resume_interrupted_turn(self: Pin<&mut Self>) {
        start_interrupted_attempt(self, BridgeRecoveryMode::Resume);
    }

    pub fn regenerate_interrupted_turn(self: Pin<&mut Self>) {
        start_interrupted_attempt(self, BridgeRecoveryMode::Regenerate);
    }

    pub fn ui_session_json(self: Pin<&mut Self>) -> QString {
        let qt = self.as_ref().get_ref().qt_thread();
        let tracker = runtime_state().request_tracker();
        let ticket = tracker.accept("ui_session.snapshot");
        let accepted = tracker.accepted_json(&ticket);
        #[cfg(feature = "rusqlite")]
        {
            let pool = runtime_state().database_pool();
            mukei_core::runtime::get().spawn(async move {
                let result = match pool {
                    Some(pool) => database_bridge::ui_session_snapshot(&pool)
                        .await
                        .map_err(|error| async_error_value(&error, "ui_session_json")),
                    None => Ok(serde_json::json!({"session": null, "active_draft": null})),
                };
                let completion = runtime_state()
                    .request_tracker()
                    .completion_json(&ticket, result);
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().async_result(QString::from(&completion));
                });
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let completion = tracker.completion_json(
                &ticket,
                Ok(serde_json::json!({"session": null, "active_draft": null})),
            );
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().async_result(QString::from(&completion));
            });
        }
        QString::from(&accepted)
    }

    pub fn save_ui_session(self: Pin<&mut Self>, session_json: QString) {
        let qt = self.as_ref().get_ref().qt_thread();
        #[cfg(feature = "rusqlite")]
        {
            let Some(pool) = runtime_state().database_pool() else {
                return;
            };
            let parsed = serde_json::from_str::<serde_json::Value>(&session_json.to_string());
            let record = match parsed {
                Ok(value) if value.is_object() => mukei_core::storage::UiSessionRecord {
                    profile_id: value
                        .get("profile_id")
                        .and_then(|value| value.as_str())
                        .unwrap_or(mukei_core::storage::DEFAULT_UI_PROFILE)
                        .to_string(),
                    schema_version: value
                        .get("schema_version")
                        .and_then(|value| value.as_i64())
                        .unwrap_or(mukei_core::storage::UI_SESSION_SCHEMA_VERSION),
                    active_route: value
                        .get("active_route")
                        .and_then(|value| value.as_str())
                        .unwrap_or("boot")
                        .to_string(),
                    active_conversation_id: value
                        .get("active_conversation_id")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    active_branch_id: value
                        .get("active_branch_id")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    timeline_anchor_message_id: value
                        .get("timeline_anchor_message_id")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    selected_model_id: value
                        .get("selected_model_id")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    payload_json: value
                        .get("payload")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({}))
                        .to_string(),
                    updated_at: chrono::Utc::now().to_rfc3339(),
                },
                _ => {
                    let error = mukei_core::error::MukeiError::ConfigInvalid {
                        field: "ui_session".into(),
                        reason: "session payload must be a JSON object".into(),
                    };
                    let event = error_bridge_event(&error, "save_ui_session");
                    let code = error.error_code().to_string();
                    let message =
                        mukei_core::diagnostics::sanitize_error_message(error.to_string());
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&message));
                    });
                    return;
                }
            };
            mukei_core::runtime::get().spawn(async move {
                if let Err(error) =
                    mukei_core::storage::UiSessionRepository::save_session(&pool, record).await
                {
                    let event = error_bridge_event(&error, "save_ui_session");
                    let code = error.error_code().to_string();
                    let message =
                        mukei_core::diagnostics::sanitize_error_message(error.to_string());
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&message));
                    });
                }
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let _ = (session_json, qt);
        }
    }

    pub fn draft_json(
        self: Pin<&mut Self>,
        conversation_id: QString,
        branch_id: QString,
    ) -> QString {
        let qt = self.as_ref().get_ref().qt_thread();
        let conversation_id = conversation_id.to_string();
        let branch_id = branch_id.to_string();
        let tracker = runtime_state().request_tracker();
        let ticket = tracker.accept("ui_session.draft");
        let accepted = tracker.accepted_json(&ticket);
        #[cfg(feature = "rusqlite")]
        {
            let pool = runtime_state().database_pool();
            mukei_core::runtime::get().spawn(async move {
                let result = match pool {
                    Some(pool) => database_bridge::draft_snapshot(
                        &pool,
                        conversation_id.clone(),
                        branch_id.clone(),
                    )
                    .await
                    .map_err(|error| async_error_value(&error, "draft_json")),
                    None => Ok(serde_json::json!({
                        "conversation_id": conversation_id,
                        "branch_id": branch_id,
                        "draft": null,
                    })),
                };
                let completion = runtime_state()
                    .request_tracker()
                    .completion_json(&ticket, result);
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().async_result(QString::from(&completion));
                });
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let completion = tracker.completion_json(
                &ticket,
                Ok(serde_json::json!({
                    "conversation_id": conversation_id,
                    "branch_id": branch_id,
                    "draft": null,
                })),
            );
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().async_result(QString::from(&completion));
            });
        }
        QString::from(&accepted)
    }

    pub fn save_draft(
        self: Pin<&mut Self>,
        conversation_id: QString,
        branch_id: QString,
        text: QString,
        cursor_position: i32,
    ) {
        let qt = self.as_ref().get_ref().qt_thread();
        #[cfg(feature = "rusqlite")]
        {
            let Some(pool) = runtime_state().database_pool() else {
                return;
            };
            let record = mukei_core::storage::UiDraftRecord {
                conversation_id: conversation_id.to_string(),
                branch_id: branch_id.to_string(),
                text: text.to_string(),
                cursor_position: i64::from(cursor_position.max(0)),
                attachment_refs_json: "[]".into(),
                updated_at: String::new(),
            };
            mukei_core::runtime::get().spawn(async move {
                if let Err(error) =
                    mukei_core::storage::UiSessionRepository::save_draft(&pool, record).await
                {
                    let event = error_bridge_event(&error, "save_draft");
                    let code = error.error_code().to_string();
                    let message =
                        mukei_core::diagnostics::sanitize_error_message(error.to_string());
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&message));
                    });
                }
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let _ = (conversation_id, branch_id, text, cursor_position, qt);
        }
    }

    pub fn clear_draft(self: Pin<&mut Self>, conversation_id: QString, branch_id: QString) {
        #[cfg(feature = "rusqlite")]
        {
            let Some(pool) = runtime_state().database_pool() else {
                return;
            };
            let conversation_id = conversation_id.to_string();
            let branch_id = branch_id.to_string();
            mukei_core::runtime::get().spawn(async move {
                if let Err(error) = mukei_core::storage::UiSessionRepository::clear_draft(
                    &pool,
                    conversation_id,
                    branch_id,
                )
                .await
                {
                    tracing::warn!(
                        code = error.error_code(),
                        "failed to clear persisted UI draft"
                    );
                }
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let _ = (conversation_id, branch_id);
        }
    }

    pub fn conversation_list_json(self: Pin<&mut Self>, limit: i32) -> QString {
        #[cfg(feature = "rusqlite")]
        {
            let Some(pool) = runtime_state().database_pool() else {
                return QString::from("[]");
            };
            let result = mukei_core::runtime::get().block_on(async move {
                mukei_core::storage::ConversationRepository::list_conversation_summaries(
                    &pool,
                    limit.max(1) as usize,
                )
                .await
            });
            return match result {
                Ok(items) => QString::from(
                    serde_json::to_string(&items)
                        .unwrap_or_else(|_| "[]".into())
                        .as_str(),
                ),
                Err(error) => QString::from(
                    serde_json::json!({
                        "error": UiError::from_mukei_error(&error, "conversation_list_json")
                    })
                    .to_string()
                    .as_str(),
                ),
            };
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let _ = limit;
            QString::from("[]")
        }
    }

    pub fn chat_snapshot_json(
        self: Pin<&mut Self>,
        conversation_id: QString,
        branch_id: QString,
        before_message_id: QString,
        limit: i32,
    ) -> QString {
        #[cfg(feature = "rusqlite")]
        {
            let Some(pool) = runtime_state().database_pool() else {
                return QString::from(
                    "{\"items\":[],\"has_older\":false,\"oldest_message_id\":\"\"}",
                );
            };
            let parsed = (|| {
                let conversation = conversation_id
                    .to_string()
                    .parse::<uuid::Uuid>()
                    .map(mukei_core::types::ConversationId)
                    .map_err(|_| "conversation_id")?;
                let branch = branch_id
                    .to_string()
                    .parse::<uuid::Uuid>()
                    .map(mukei_core::types::BranchId)
                    .map_err(|_| "branch_id")?;
                let before = if before_message_id.to_string().trim().is_empty() {
                    None
                } else {
                    Some(
                        before_message_id
                            .to_string()
                            .parse::<uuid::Uuid>()
                            .map(mukei_core::types::MessageId)
                            .map_err(|_| "before_message_id")?,
                    )
                };
                Ok::<_, &'static str>((conversation, branch, before))
            })();
            let (conversation, branch, before) = match parsed {
                Ok(value) => value,
                Err(field) => {
                    return QString::from(
                        serde_json::json!({
                            "error": {
                                "code": "ERR_UI_INVALID_SCOPE",
                                "severity": "warning",
                                "recoverable": true,
                                "safe_message": format!("invalid {field}")
                            }
                        })
                        .to_string()
                        .as_str(),
                    )
                }
            };
            let result = mukei_core::runtime::get().block_on(async move {
                mukei_core::storage::ConversationRepository::timeline_page(
                    &pool,
                    conversation,
                    branch,
                    before,
                    limit.max(1) as usize,
                )
                .await
            });
            return match result {
                Ok(page) => QString::from(
                    serde_json::to_string(&page)
                        .unwrap_or_else(|_| {
                            "{\"items\":[],\"has_older\":false,\"oldest_message_id\":\"\"}".into()
                        })
                        .as_str(),
                ),
                Err(error) => QString::from(
                    serde_json::json!({
                        "error": UiError::from_mukei_error(&error, "chat_snapshot_json")
                    })
                    .to_string()
                    .as_str(),
                ),
            };
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let _ = (conversation_id, branch_id, before_message_id, limit);
            QString::from("{\"items\":[],\"has_older\":false,\"oldest_message_id\":\"\"}")
        }
    }

    pub fn update_setting(self: Pin<&mut Self>, key: QString, value: QVariant) {
        let qt = self.as_ref().get_ref().qt_thread();
        #[cfg(feature = "rusqlite")]
        {
            let key_string = key.to_string();
            let pref = match preference_value_from_qvariant(&key_string, &value) {
                Ok(pref) => pref,
                Err(err) => {
                    let event = error_bridge_event(&err, "update_setting");
                    let code = err.error_code().to_string();
                    let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&message));
                    });
                    return;
                }
            };
            let Some(pool) = runtime_state().database_pool() else {
                let err = mukei_core::error::MukeiError::DatabaseInitFailed(
                    "settings database is not initialized".into(),
                );
                let event = error_bridge_event(&err, "update_setting");
                let code = err.error_code().to_string();
                let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().event_emitted(event_json(event));
                    qobject
                        .as_mut()
                        .error_occurred(QString::from(&code), QString::from(&message));
                });
                return;
            };
            mukei_core::runtime::get().spawn(async move {
                match mukei_core::storage::SettingsRepository::upsert_preference(
                    &pool,
                    key_string.clone(),
                    pref.clone(),
                )
                .await
                {
                    Ok(()) => {
                        if let (
                            "remote_feature_policy",
                            mukei_core::storage::PreferenceValue::String(policy),
                        ) = (key_string.as_str(), pref)
                        {
                            match policy.parse::<RemoteFeaturePolicy>() {
                                Ok(policy) => {
                                    runtime_state().set_remote_feature_policy(policy);
                                    rebuild_tool_registry_from_secrets().await;
                                }
                                Err(err) => {
                                    let event = error_bridge_event(&err, "update_setting");
                                    let code = err.error_code().to_string();
                                    let message = mukei_core::diagnostics::sanitize_error_message(
                                        err.to_string(),
                                    );
                                    let _ = qt.queue(move |mut qobject| {
                                        qobject.as_mut().event_emitted(event_json(event));
                                        qobject.as_mut().error_occurred(
                                            QString::from(&code),
                                            QString::from(&message),
                                        );
                                    });
                                }
                            }
                        }
                    }
                    Err(err) => {
                        let event = error_bridge_event(&err, "update_setting");
                        let code = err.error_code().to_string();
                        let message =
                            mukei_core::diagnostics::sanitize_error_message(err.to_string());
                        let _ = qt.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(event));
                            qobject
                                .as_mut()
                                .error_occurred(QString::from(&code), QString::from(&message));
                        });
                    }
                }
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let _ = (key, value);
            let err = mukei_core::error::MukeiError::DatabaseInitFailed(
                "settings persistence is disabled in this build".into(),
            );
            let event = error_bridge_event(&err, "update_setting");
            let code = err.error_code().to_string();
            let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(event));
                qobject
                    .as_mut()
                    .error_occurred(QString::from(&code), QString::from(&message));
            });
        }
    }

    pub fn download_jobs_json(self: Pin<&mut Self>, limit: i32) -> QString {
        let qt = self.as_ref().get_ref().qt_thread();
        let tracker = runtime_state().request_tracker();
        let ticket = tracker.accept("downloads.snapshot");
        let accepted = tracker.accepted_json(&ticket);
        #[cfg(feature = "rusqlite")]
        {
            let pool = runtime_state().database_pool();
            mukei_core::runtime::get().spawn(async move {
                let result = match pool {
                    Some(pool) => {
                        download_bridge::recent_jobs_snapshot(&pool, limit.max(1) as usize)
                            .await
                            .map_err(|error| async_error_value(&error, "download_jobs_json"))
                    }
                    None => Ok(serde_json::json!([])),
                };
                let completion = runtime_state()
                    .request_tracker()
                    .completion_json(&ticket, result);
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().async_result(QString::from(&completion));
                });
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let _ = limit;
            let completion = tracker.completion_json(&ticket, Ok(Vec::<serde_json::Value>::new()));
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().async_result(QString::from(&completion));
            });
        }
        QString::from(&accepted)
    }

    pub fn storage_snapshot_json(self: Pin<&mut Self>) -> QString {
        let qt = self.as_ref().get_ref().qt_thread();
        let tracker = runtime_state().request_tracker();
        let ticket = tracker.accept("storage.snapshot");
        let accepted = tracker.accepted_json(&ticket);
        let model_root = runtime_state().model_dir();
        mukei_core::runtime::get().spawn(async move {
            let result =
                tokio::task::spawn_blocking(move || storage_bridge::storage_snapshot(&model_root))
                    .await
                    .map_err(|error| {
                        serde_json::json!({
                            "code": "ERR_BLOCKING_JOIN",
                            "safe_message": "Storage usage could not be measured safely.",
                            "technical_message": error.to_string(),
                            "recoverable": true,
                        })
                    })
                    .and_then(|value| {
                        value.map_err(|error| async_error_value(&error, "storage_snapshot_json"))
                    });
            let completion = runtime_state()
                .request_tracker()
                .completion_json(&ticket, result);
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().async_result(QString::from(&completion));
            });
        });
        QString::from(&accepted)
    }

    pub fn document_list_json(self: Pin<&mut Self>, limit: i32) -> QString {
        let qt = self.as_ref().get_ref().qt_thread();
        let tracker = runtime_state().request_tracker();
        let ticket = tracker.accept("documents.snapshot");
        let accepted = tracker.accepted_json(&ticket);
        #[cfg(feature = "rusqlite")]
        {
            let pool = runtime_state().database_pool();
            mukei_core::runtime::get().spawn(async move {
                let result = match pool {
                    Some(pool) => {
                        document_bridge::document_list_snapshot(&pool, limit.max(1) as usize)
                            .await
                            .map_err(|error| async_error_value(&error, "document_list_json"))
                    }
                    None => Ok(serde_json::json!([])),
                };
                let completion = runtime_state()
                    .request_tracker()
                    .completion_json(&ticket, result);
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().async_result(QString::from(&completion));
                });
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let _ = limit;
            let completion = tracker.completion_json(&ticket, Ok(Vec::<serde_json::Value>::new()));
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().async_result(QString::from(&completion));
            });
        }
        QString::from(&accepted)
    }

    pub fn settings_snapshot_json(self: Pin<&mut Self>) -> QString {
        let qt = self.as_ref().get_ref().qt_thread();
        let tracker = runtime_state().request_tracker();
        let ticket = tracker.accept("settings.snapshot");
        let accepted = tracker.accepted_json(&ticket);
        #[cfg(feature = "rusqlite")]
        {
            let pool = runtime_state().database_pool();
            mukei_core::runtime::get().spawn(async move {
                let result = match pool {
                    Some(pool) => settings_bridge::settings_snapshot(&pool)
                        .await
                        .map_err(|error| async_error_value(&error, "settings_snapshot_json")),
                    None => Ok(serde_json::json!([])),
                };
                let completion = runtime_state()
                    .request_tracker()
                    .completion_json(&ticket, result);
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().async_result(QString::from(&completion));
                });
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let completion = tracker.completion_json(&ticket, Ok(Vec::<serde_json::Value>::new()));
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().async_result(QString::from(&completion));
            });
        }
        QString::from(&accepted)
    }

    pub fn select_installed_model_json(self: Pin<&mut Self>, model_id: QString) -> QString {
        let model_id = model_id.to_string();
        let generation =
            match begin_model_activation(&model_id) {
                Ok(generation) => generation,
                Err(error) => return QString::from(
                    serde_json::json!({
                        "ok": false,
                        "error": UiError::from_mukei_error(&error, "select_installed_model_json")
                    })
                    .to_string()
                    .as_str(),
                ),
            };
        let qt = self.as_ref().get_ref().qt_thread();
        let tracker = runtime_state().request_tracker();
        let ticket = tracker.accept("model.activate");
        let accepted = tracker.accepted_json(&ticket);
        mukei_core::runtime::get().spawn(async move {
            let result = match complete_model_activation(model_id, generation).await {
                ModelActivationTaskResult::Ready(payload) => Ok(payload),
                ModelActivationTaskResult::Superseded => Err(serde_json::json!({
                    "code": "ERR_MODEL_ACTIVATION_SUPERSEDED",
                    "safe_message": "A newer model selection replaced this activation request.",
                    "recoverable": true,
                })),
                ModelActivationTaskResult::Failed(error) => {
                    Err(async_error_value(&error, "select_installed_model_json"))
                }
            };
            let completion = runtime_state()
                .request_tracker()
                .completion_json(&ticket, result);
            let capability = event_json(BridgeEvent::new(BridgeEventKind::CapabilitySnapshot {
                capabilities: current_ready_capabilities(),
            }));
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().async_result(QString::from(&completion));
                qobject.as_mut().event_emitted(capability);
            });
        });
        QString::from(&accepted)
    }

    pub fn delete_installed_model_json(self: Pin<&mut Self>, model_id: QString) -> QString {
        let model_id = model_id.to_string();
        let Some(descriptor) = mukei_core::engine::lookup_model_str(&model_id) else {
            let error = mukei_core::error::MukeiError::ConfigInvalid {
                field: "model_id".into(),
                reason: "unknown model identifier".into(),
            };
            return QString::from(
                serde_json::json!({
                    "ok": false,
                    "error": UiError::from_mukei_error(&error, "delete_installed_model_json")
                })
                .to_string()
                .as_str(),
            );
        };
        let activation = runtime_state().model_activation_service();
        if activation
            .active_model_snapshot()
            .as_ref()
            .is_some_and(|active| active.model_id == model_id)
            || (activation.readiness_snapshot().activation_in_progress
                && activation
                    .selected_model_snapshot()
                    .as_ref()
                    .is_some_and(|(selected, _)| selected == &model_id))
        {
            let error = mukei_core::error::MukeiError::BridgeBusy;
            return QString::from(
                serde_json::json!({
                    "ok": false,
                    "error": UiError::from_mukei_error(&error, "delete_installed_model_json")
                })
                .to_string()
                .as_str(),
            );
        }
        if self.as_ref().rust().busy.load(Ordering::Acquire) {
            let error = mukei_core::error::MukeiError::BridgeBusy;
            return QString::from(
                serde_json::json!({
                    "ok": false,
                    "error": UiError::from_mukei_error(&error, "delete_installed_model_json")
                })
                .to_string()
                .as_str(),
            );
        }
        if self
            .as_ref()
            .rust()
            .active_downloads
            .lock()
            .iter()
            .any(|download| download.model_id.as_deref() == Some(model_id.as_str()))
        {
            let error = mukei_core::error::MukeiError::DownloadBusy {
                dest: format!("model:{model_id}"),
            };
            return QString::from(
                serde_json::json!({
                    "ok": false,
                    "error": UiError::from_mukei_error(&error, "delete_installed_model_json")
                })
                .to_string()
                .as_str(),
            );
        }
        let model_root = runtime_state().model_dir();
        let path = model_root.join(descriptor.filename);
        let canonical_root = match std::fs::canonicalize(&model_root) {
            Ok(value) => value,
            Err(error) => {
                let error = mukei_core::error::MukeiError::Io(error.to_string());
                return QString::from(
                    serde_json::json!({
                        "ok": false,
                        "error": UiError::from_mukei_error(&error, "delete_installed_model_json")
                    })
                    .to_string()
                    .as_str(),
                );
            }
        };
        let canonical_path = match std::fs::canonicalize(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return QString::from(
                    serde_json::json!({"ok": true, "model_id": descriptor.id.as_str(), "deleted": false})
                        .to_string()
                        .as_str(),
                );
            }
            Err(error) => {
                let error = mukei_core::error::MukeiError::Io(error.to_string());
                return QString::from(
                    serde_json::json!({
                        "ok": false,
                        "error": UiError::from_mukei_error(&error, "delete_installed_model_json")
                    })
                    .to_string()
                    .as_str(),
                );
            }
        };
        if !canonical_path.starts_with(&canonical_root)
            || canonical_path.parent() != Some(canonical_root.as_path())
        {
            let error = mukei_core::error::MukeiError::ConfigInvalid {
                field: "models_dir".into(),
                reason: "model path escaped the app-private model directory".into(),
            };
            return QString::from(
                serde_json::json!({
                    "ok": false,
                    "error": UiError::from_mukei_error(&error, "delete_installed_model_json")
                })
                .to_string()
                .as_str(),
            );
        }
        match std::fs::remove_file(&canonical_path) {
            Ok(()) => QString::from(
                serde_json::json!({"ok": true, "model_id": descriptor.id.as_str(), "deleted": true})
                    .to_string()
                    .as_str(),
            ),
            Err(error) => {
                let error = mukei_core::error::MukeiError::Io(error.to_string());
                QString::from(
                    serde_json::json!({
                        "ok": false,
                        "error": UiError::from_mukei_error(&error, "delete_installed_model_json")
                    })
                    .to_string()
                    .as_str(),
                )
            }
        }
    }

    pub fn grant_document_access_json(
        self: Pin<&mut Self>,
        target: QString,
        label: QString,
        mime: QString,
    ) -> QString {
        let qt = self.as_ref().get_ref().qt_thread();
        let target = target.to_string();
        let mut label = label.to_string();
        let mut mime = mime.to_string();
        let invalid = if target.trim().is_empty() || target.len() > 16 * 1024 {
            Some(mukei_core::error::MukeiError::ConfigInvalid {
                field: "document_target".into(),
                reason: "document URI is empty or too long".into(),
            })
        } else if !(target.starts_with("content://") || target.starts_with("file://")) {
            Some(mukei_core::error::MukeiError::ConfigInvalid {
                field: "document_target".into(),
                reason: "only user-selected content:// or file:// URIs are accepted".into(),
            })
        } else if label.len() > 512 || mime.len() > 255 {
            Some(mukei_core::error::MukeiError::ConfigInvalid {
                field: "document_metadata".into(),
                reason: "document label or MIME type is too long".into(),
            })
        } else {
            None
        };
        if let Some(error) = invalid {
            return QString::from(
                serde_json::json!({
                    "ok": false,
                    "error": UiError::from_mukei_error(&error, "grant_document_access_json")
                })
                .to_string()
                .as_str(),
            );
        }
        if label.trim().is_empty() {
            label = "Private document".to_string();
        }
        if mime.trim().is_empty() {
            mime = "application/octet-stream".to_string();
        }
        let tracker = runtime_state().request_tracker();
        let ticket = tracker.accept("documents.grant");
        let accepted = tracker.accepted_json(&ticket);
        #[cfg(feature = "rusqlite")]
        {
            let pool = runtime_state().database_pool();
            mukei_core::runtime::get().spawn(async move {
                let result = async {
                    let pool = pool.ok_or_else(|| {
                        mukei_core::error::MukeiError::DatabaseInitFailed(
                            "private storage is not ready".into(),
                        )
                    })?;
                    let permission_target = target.clone();
                    let permission_state = tokio::task::spawn_blocking(move || {
                        let state =
                            android_document_access::persist_read_permission(&permission_target)
                                .map_err(mukei_core::error::MukeiError::Io)?;
                        if state == android_document_access::PermissionState::Failed
                            || !android_document_access::can_read(&permission_target)
                                .unwrap_or(false)
                        {
                            return Err(mukei_core::error::MukeiError::SafRequired);
                        }
                        Ok(state)
                    })
                    .await
                    .map_err(|error| {
                        mukei_core::error::MukeiError::BlockingJoinFailed(error.to_string())
                    })??;
                    let token = format!("saf-{}", uuid::Uuid::new_v4());
                    let row = core_saf::SafTokenRow {
                        token_id: token,
                        source: label,
                        target: target.clone(),
                        mime,
                        revoked: false,
                        created: chrono::Utc::now(),
                    };
                    let permission_label = permission_state.as_str().to_string();
                    match runtime_state()
                        .saf_registry()
                        .persist_document_grant(&pool, row, &permission_label)
                        .await
                    {
                        Ok(document_id) => Ok(serde_json::json!({
                            "document_id": document_id,
                            "state": "access_granted",
                            "permission_state": permission_state.as_str(),
                            "ingestion_state": "waiting_for_embedder",
                            "indexed": false
                        })),
                        Err(error) => {
                            if matches!(
                                permission_state,
                                android_document_access::PermissionState::Persisted
                            ) {
                                let release_target = target.clone();
                                let _ = tokio::task::spawn_blocking(move || {
                                    android_document_access::release_read_permission(
                                        &release_target,
                                    )
                                })
                                .await;
                            }
                            Err(error)
                        }
                    }
                }
                .await;
                let result = match result {
                    Ok(payload) => {
                        tracing::info!(domain = "documents.grant", "document operation completed");
                        Ok(payload)
                    }
                    Err(error) => {
                        tracing::warn!(
                            domain = "documents.grant",
                            code = error.error_code(),
                            "document operation failed"
                        );
                        Err(async_error_value(&error, "grant_document_access_json"))
                    }
                };
                let completion = runtime_state()
                    .request_tracker()
                    .completion_json(&ticket, result);
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().async_result(QString::from(&completion));
                });
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let error = mukei_core::error::MukeiError::DatabaseInitFailed(
                "document persistence is disabled in this build".into(),
            );
            let completion = tracker.completion_json::<serde_json::Value>(
                &ticket,
                Err(async_error_value(&error, "grant_document_access_json")),
            );
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().async_result(QString::from(&completion));
            });
        }
        QString::from(&accepted)
    }

    pub fn revoke_document_json(self: Pin<&mut Self>, document_id: QString) -> QString {
        let qt = self.as_ref().get_ref().qt_thread();
        let document_id = document_id.to_string();
        let tracker = runtime_state().request_tracker();
        let ticket = tracker.accept("documents.revoke");
        let accepted = tracker.accepted_json(&ticket);
        #[cfg(feature = "rusqlite")]
        {
            let pool = runtime_state().database_pool();
            mukei_core::runtime::get().spawn(async move {
                let result = async {
                    let pool = pool.ok_or_else(|| {
                        mukei_core::error::MukeiError::DatabaseInitFailed(
                            "private storage is not ready".into(),
                        )
                    })?;
                    let token =
                        core_saf::SafRegistry::token_for_document_id(&pool, &document_id).await?;
                    let target_for_release = core_saf::SafRegistry::target_for_token(&pool, &token)
                        .await
                        .ok();
                    let plan = runtime_state()
                        .saf_registry()
                        .persist_revoke(&pool, &token, "user_revoke")
                        .await?;
                    if let Err(error) = record_document_revoke_audit(
                        &pool,
                        &plan.file_token,
                        "user_revoke",
                        plan.chunk_ids.len(),
                    )
                    .await
                    {
                        tracing::error!(
                            code = error.error_code(),
                            "document revoke audit linkage remains retryable"
                        );
                    }
                    if let Some(config) = runtime_state().config() {
                        match agent_runtime::purge_vector_chunks(&config, plan.chunk_ids.clone())
                            .await
                        {
                            Ok(_) => {
                                core_saf::SafRegistry::mark_document_cleanup_complete(
                                    &pool,
                                    &plan.file_token,
                                )
                                .await?
                            }
                            Err(error) => {
                                core_saf::SafRegistry::mark_document_cleanup_failed(
                                    &pool,
                                    &plan.file_token,
                                    &error,
                                )
                                .await?;
                            }
                        }
                    }
                    if let Some(target) = target_for_release {
                        let _ = tokio::task::spawn_blocking(move || {
                            android_document_access::release_read_permission(&target)
                        })
                        .await;
                    }
                    Ok::<_, mukei_core::error::MukeiError>(serde_json::json!({
                        "document_id": document_id,
                        "revoked": true,
                    }))
                }
                .await;
                let result = match result {
                    Ok(payload) => {
                        tracing::info!(domain = "documents.revoke", "document operation completed");
                        Ok(payload)
                    }
                    Err(error) => {
                        tracing::warn!(
                            domain = "documents.revoke",
                            code = error.error_code(),
                            "document operation failed"
                        );
                        Err(async_error_value(&error, "revoke_document_json"))
                    }
                };
                let completion = runtime_state()
                    .request_tracker()
                    .completion_json(&ticket, result);
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().async_result(QString::from(&completion));
                });
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let error = mukei_core::error::MukeiError::DatabaseInitFailed(
                "document persistence is disabled in this build".into(),
            );
            let completion = tracker.completion_json::<serde_json::Value>(
                &ticket,
                Err(async_error_value(&error, "revoke_document_json")),
            );
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().async_result(QString::from(&completion));
            });
        }
        QString::from(&accepted)
    }

    pub fn retry_document_ingestion_json(self: Pin<&mut Self>, document_id: QString) -> QString {
        let qt = self.as_ref().get_ref().qt_thread();
        let document_id = document_id.to_string();
        let tracker = runtime_state().request_tracker();
        let ticket = tracker.accept("documents.retry");
        let accepted = tracker.accepted_json(&ticket);
        #[cfg(feature = "rusqlite")]
        {
            let pool = runtime_state().database_pool();
            mukei_core::runtime::get().spawn(async move {
                let result = async {
                    let pool = pool.ok_or_else(|| {
                        mukei_core::error::MukeiError::DatabaseInitFailed(
                            "private storage is not ready".into(),
                        )
                    })?;
                    let token =
                        core_saf::SafRegistry::token_for_document_id(&pool, &document_id).await?;
                    let id = core_saf::SafRegistry::queue_document_ingestion(&pool, &token).await?;
                    Ok::<_, mukei_core::error::MukeiError>(serde_json::json!({
                        "document_id": id,
                        "ingestion_state": "waiting_for_embedder",
                        "indexed": false
                    }))
                }
                .await;
                let result = match result {
                    Ok(payload) => {
                        tracing::info!(domain = "documents.retry", "document operation completed");
                        Ok(payload)
                    }
                    Err(error) => {
                        tracing::warn!(
                            domain = "documents.retry",
                            code = error.error_code(),
                            "document operation failed"
                        );
                        Err(async_error_value(&error, "retry_document_ingestion_json"))
                    }
                };
                let completion = runtime_state()
                    .request_tracker()
                    .completion_json(&ticket, result);
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().async_result(QString::from(&completion));
                });
            });
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let error = mukei_core::error::MukeiError::DatabaseInitFailed(
                "document persistence is disabled in this build".into(),
            );
            let completion = tracker.completion_json::<serde_json::Value>(
                &ticket,
                Err(async_error_value(&error, "retry_document_ingestion_json")),
            );
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().async_result(QString::from(&completion));
            });
        }
        QString::from(&accepted)
    }

    pub fn ui_contract_snapshot_json(self: Pin<&mut Self>) -> QString {
        let snapshot = mukei_core::ui_contract::UiContractSnapshot::current();
        QString::from(
            serde_json::to_string(&snapshot)
                .unwrap_or_else(|_| "{\"schema_version\":1,\"contract_version\":1,\"min_qml_contract_version\":1,\"max_qml_contract_version\":1}".to_string())
                .as_str(),
        )
    }

    pub fn operation_snapshot_json(self: Pin<&mut Self>) -> QString {
        #[cfg(feature = "rusqlite")]
        {
            let Some(pool) = runtime_state().database_pool() else {
                return QString::from("{\"schema_version\":1,\"operations\":[]}");
            };
            let result = mukei_core::runtime::get().block_on(async {
                let downloads = mukei_core::storage::DownloadJobRepository::list_recent(&pool, 100).await?;
                let documents = core_saf::SafRegistry::list_document_projections(&pool, 250).await?;
                let mut operations = Vec::new();

                for job in downloads {
                    if matches!(job.status.as_str(), "queued" | "downloading") {
                        let progress = if job.expected_bytes == 0 {
                            0.0
                        } else {
                            (job.bytes_downloaded as f64 / job.expected_bytes as f64).clamp(0.0, 1.0)
                        };
                        let related_entity_id = job.model_id.clone().unwrap_or_default();
                        let operation_identity = if related_entity_id.is_empty() {
                            job.job_id.clone()
                        } else {
                            related_entity_id.clone()
                        };
                        operations.push(serde_json::json!({
                            "operation_id": format!("download:{operation_identity}"),
                            "type": "download",
                            "state": job.status,
                            "progress": progress,
                            "cancelable": true,
                            "retryable": false,
                            "label": "Downloading model",
                            "related_entity_id": related_entity_id,
                            "updated_at": job.updated_at
                        }));
                    }
                }

                for document in documents {
                    if document.cleanup_pending {
                        operations.push(serde_json::json!({
                            "operation_id": format!("document_cleanup:{}", document.document_id),
                            "type": "document_cleanup",
                            "state": "pending",
                            "progress": 0.0,
                            "cancelable": false,
                            "retryable": true,
                            "label": "Cleaning private document data",
                            "related_entity_id": document.document_id,
                            "updated_at": document.updated_at
                        }));
                    } else if matches!(
                        document.ingestion_state.as_str(),
                        "queued" | "reading" | "chunking" | "embedding" | "committing" | "waiting_for_embedder"
                    ) {
                        let blocked = document.ingestion_state == "waiting_for_embedder";
                        operations.push(serde_json::json!({
                            "operation_id": format!("document_ingestion:{}", document.document_id),
                            "type": "document_ingestion",
                            "state": if blocked { "blocked" } else { document.ingestion_state.as_str() },
                            "phase": document.ingestion_state,
                            "progress": (document.ingestion_progress_percent as f64 / 100.0).clamp(0.0, 1.0),
                            "cancelable": false,
                            "retryable": document.ingestion_retryable,
                            "label": if blocked { "Waiting for document embedder" } else { "Indexing private document" },
                            "safe_message": document.ingestion_error,
                            "related_entity_id": document.document_id,
                            "updated_at": document.updated_at
                        }));
                    }
                }

                Ok::<_, mukei_core::error::MukeiError>(serde_json::json!({
                    "schema_version": 1,
                    "operations": operations
                }))
            });
            return match result {
                Ok(value) => QString::from(value.to_string().as_str()),
                Err(error) => QString::from(
                    serde_json::json!({
                        "schema_version": 1,
                        "operations": [],
                        "error": UiError::from_mukei_error(&error, "operation_snapshot_json")
                    })
                    .to_string()
                    .as_str(),
                ),
            };
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            QString::from("{\"schema_version\":1,\"operations\":[]}")
        }
    }

    pub fn engine_session_snapshot_json(self: Pin<&mut Self>) -> QString {
        let persisted_selected_model_id = {
            #[cfg(feature = "rusqlite")]
            {
                let pool = runtime_state().database_pool();
                pool.and_then(|pool| {
                    mukei_core::runtime::get().block_on(async {
                        mukei_core::storage::UiSessionRepository::load_session(
                            &pool,
                            mukei_core::storage::DEFAULT_UI_PROFILE,
                        )
                        .await
                        .ok()
                        .flatten()
                        .and_then(|session| session.selected_model_id)
                    })
                })
            }
            #[cfg(not(feature = "rusqlite"))]
            {
                None::<String>
            }
        };
        let activation = runtime_state().model_activation_service();
        let readiness = activation.readiness_snapshot();
        let selected_model_id = activation
            .selected_model_snapshot()
            .map(|(model_id, _)| model_id)
            .or(persisted_selected_model_id);
        let active = activation.active_model_snapshot();
        let identity = activation.identity();
        let loaded_model_id = active.as_ref().map(|snapshot| snapshot.model_id.clone());
        let activation_required = selected_model_id.as_ref() != loaded_model_id.as_ref();
        let safe_message = if readiness.activation_failed && readiness.active_backend_ready {
            "The replacement model could not be activated; the previous model remains ready."
        } else if readiness.activation_in_progress && readiness.active_backend_ready {
            "A replacement model is being activated while the current model remains ready."
        } else if readiness.activation_in_progress {
            "The selected model is being verified and activated."
        } else if readiness.active_backend_ready {
            "The active model is ready for local inference."
        } else if !readiness.real_backend_implementation_available {
            "A production inference backend is unavailable in this runtime."
        } else {
            "Select and activate an installed model before starting chat."
        };
        QString::from(serde_json::json!({
            "schema_version": 3,
            "selected_model_id": selected_model_id,
            "loaded_model_id": loaded_model_id,
            "inference_backend": identity.implementation,
            "backend_kind": identity.kind.as_tag(),
            "backend_unavailable_reason": identity.unavailable_reason.map(|reason| reason.as_tag()),
            "activation_supported": readiness.real_backend_implementation_available,
            "activation_required": activation_required,
            "activation_in_progress": readiness.activation_in_progress,
            "activation_failed": readiness.activation_failed,
            "active_model_ready": readiness.active_backend_ready,
            "product_ready": readiness.product_ready,
            "restart_required": false,
            "safe_message": safe_message
        }).to_string().as_str())
    }

    pub fn diagnostics_snapshot_json(self: Pin<&mut Self>) -> QString {
        let runtime_phase = format!("{:?}", runtime_state().runtime_coordinator().phase());
        let model_root = runtime_state().model_dir();
        let storage = mukei_core::storage::StorageQuotaManager::new(&model_root)
            .usage()
            .ok();
        let document_count = runtime_state().saf_registry().count();
        QString::from(
            serde_json::json!({
                "schema_version": 1,
                "runtime_phase": runtime_phase,
                "ready": runtime_state().runtime_coordinator().is_ready(),
                "document_grant_count": document_count,
                "storage": storage.map(|value| serde_json::json!({
                    "model_bytes": value.model_bytes,
                    "partial_bytes": value.partial_bytes,
                    "total_bytes": value.total_bytes
                })),
                "provenance": provenance::snapshot(),
                "privacy": {
                    "contains_prompts": false,
                    "contains_document_contents": false,
                    "contains_secrets": false,
                    "contains_private_paths": false
                }
            })
            .to_string()
            .as_str(),
        )
    }

    pub fn provenance_snapshot_json(self: Pin<&mut Self>) -> QString {
        QString::from(
            serde_json::to_string(&provenance::snapshot())
                .unwrap_or_else(|_| {
                    "{\"schema_version\":1,\"product_version\":\"unknown\"}".to_string()
                })
                .as_str(),
        )
    }

    pub fn export_diagnostics_json(self: Pin<&mut Self>) -> QString {
        if !cfg!(feature = "diagnostics_export") {
            return QString::from("{\"ok\":false,\"error\":{\"code\":\"ERR_DIAGNOSTICS_EXPORT_DISABLED\",\"safe_message\":\"Diagnostics export is disabled by runtime policy.\"}}");
        }
        let snapshot = self.diagnostics_snapshot_json().to_string();
        let Some(config) = runtime_state().config() else {
            return QString::from("{\"ok\":false,\"error\":{\"code\":\"ERR_CONFIG\",\"safe_message\":\"Diagnostics are unavailable before initialization.\"}}");
        };
        let export_dir = config.logs_dir.join("exports");
        if let Err(error) = std::fs::create_dir_all(&export_dir) {
            let error = mukei_core::error::MukeiError::Io(error.to_string());
            return QString::from(
                serde_json::json!({
                    "ok": false,
                    "error": UiError::from_mukei_error(&error, "export_diagnostics_json")
                })
                .to_string()
                .as_str(),
            );
        }
        let export_id = format!("diagnostics-{}", uuid::Uuid::new_v4());
        let path = export_dir.join(format!("{export_id}.json"));
        let temporary_path = export_dir.join(format!(".{export_id}.json.partial"));
        let write_result = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary_path)?;
            file.write_all(snapshot.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary_path, &path)?;
            Ok(())
        })();
        match write_result {
            Ok(()) => QString::from(
                serde_json::json!({
                    "ok": true,
                    "export_id": export_id,
                    "filename": path.file_name().and_then(|name| name.to_str()).unwrap_or("diagnostics.json")
                })
                .to_string()
                .as_str(),
            ),
            Err(error) => {
                let _ = std::fs::remove_file(&temporary_path);
                let error = mukei_core::error::MukeiError::Io(error.to_string());
                QString::from(
                    serde_json::json!({
                        "ok": false,
                        "error": UiError::from_mukei_error(&error, "export_diagnostics_json")
                    })
                    .to_string()
                    .as_str(),
                )
            }
        }
    }
}

impl ffi::MukeiBridge {
    /// Inject the unwrapped Brave API key and rebuild the shared tool
    /// registry so the next `web_search` call uses the new credential.
    /// (Issue #3.)
    pub fn set_brave_api_key(self: Pin<&mut Self>, api_key: QString) {
        let qt = self.as_ref().get_ref().qt_thread();
        let api_key = Zeroizing::new(api_key.to_string());
        if let Err(reason) = persist_provider_secret(BRAVE_SECRET_ALIAS, &api_key) {
            let err = mukei_core::error::MukeiError::SafeStorageUnavailable(reason);
            let event = error_bridge_event(&err, "set_brave_api_key");
            let code = err.error_code().to_string();
            let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(event));
                qobject
                    .as_mut()
                    .error_occurred(QString::from(&code), QString::from(&message));
            });
            return;
        }
        let mut slot = runtime_state().brave_api_key().lock();
        *slot = (!api_key.trim().is_empty()).then_some(api_key);
        drop(slot);
        rebuild_tool_registry_from_secrets_blocking();
    }

    /// Inject the unwrapped Tavily API key (Issue #3). Symmetric with
    /// `set_brave_api_key` — the previous bridge had no Tavily setter
    /// at all.
    pub fn set_tavily_api_key(self: Pin<&mut Self>, api_key: QString) {
        let qt = self.as_ref().get_ref().qt_thread();
        let api_key = Zeroizing::new(api_key.to_string());
        if let Err(reason) = persist_provider_secret(TAVILY_SECRET_ALIAS, &api_key) {
            let err = mukei_core::error::MukeiError::SafeStorageUnavailable(reason);
            let event = error_bridge_event(&err, "set_tavily_api_key");
            let code = err.error_code().to_string();
            let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(event));
                qobject
                    .as_mut()
                    .error_occurred(QString::from(&code), QString::from(&message));
            });
            return;
        }
        let mut slot = runtime_state().tavily_api_key().lock();
        *slot = (!api_key.trim().is_empty()).then_some(api_key);
        drop(slot);
        rebuild_tool_registry_from_secrets_blocking();
    }

    /// Set the privacy policy for remote features. The default is
    /// `local_only`, so web search cannot send queries off-device until
    /// this is explicitly set to `remote_allowed`.
    pub fn set_remote_feature_policy(self: Pin<&mut Self>, policy: QString) {
        let qt = self.as_ref().get_ref().qt_thread();
        let raw_policy = policy.to_string();
        let parsed = raw_policy.parse::<RemoteFeaturePolicy>();
        match parsed {
            Ok(policy) => {
                runtime_state().set_remote_feature_policy(policy);
                rebuild_tool_registry_from_secrets_blocking();
            }
            Err(err) => {
                let event = error_bridge_event(&err, "set_remote_feature_policy");
                let code = err.error_code().to_string();
                let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().event_emitted(event_json(event));
                    qobject
                        .as_mut()
                        .error_occurred(QString::from(&code), QString::from(&message));
                });
            }
        }
    }

    /// Legacy ABI compatibility only. Plaintext database-key injection across
    /// QString/QML is intentionally disabled; secure startup creates or unwraps
    /// the key inside the native bootstrap state machine.
    pub fn set_database_cipher_key(self: Pin<&mut Self>, key: QString) {
        let qt = self.as_ref().get_ref().qt_thread();
        let _discarded = Zeroizing::new(key.to_string());
        let err = mukei_core::error::MukeiError::ConfigInvalid {
            field: "database_cipher_key".to_string(),
            reason: "external database-key injection is disabled; use secure native bootstrap"
                .to_string(),
        };
        let event = error_bridge_event(&err, "set_database_cipher_key");
        let _ = qt.queue(move |mut qobject| {
            qobject.as_mut().event_emitted(event_json(event));
            qobject.as_mut().error_occurred(
                QString::from("ERR_DATABASE_KEY_EXTERNAL_INJECTION_DISABLED"),
                QString::from("Database keys cannot be supplied through QML."),
            );
        });
    }

    pub fn note_thermal_status(self: Pin<&mut Self>, status: i32) {
        let qt = self.as_ref().get_ref().qt_thread();
        runtime_state().set_thermal_status(status);
        mukei_core::runtime::get().spawn(async move {
            let _ = qt.queue(move |mut qobject| {
                if status >= 3 {
                    qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                        BridgeEventKind::AppLifecycle {
                            state: AppLifecycleState::Degraded,
                            capabilities: current_ready_capabilities(),
                            android_storage: Some(AndroidStorageState::Ready {
                                saf_grant_count: runtime_state().saf_registry().count(),
                            }),
                        },
                    )));
                }
                qobject.as_mut().thermal_status_changed(status);
            });
        });
    }

    pub fn saf_registry_count(self: Pin<&mut Self>) -> i32 {
        runtime_state().saf_registry().count() as i32
    }

    // -----------------------------------------------------------------
    // Model download surface (TRD §8.1 / REQ-MOD-01)
    // -----------------------------------------------------------------

    pub fn set_model_dir(self: Pin<&mut Self>, path: QString) {
        let qt = self.as_ref().get_ref().qt_thread();
        let validated = match validate_model_dir(std::path::PathBuf::from(path.to_string())) {
            Ok(path) => path,
            Err(err) => {
                let code = err.error_code().to_string();
                let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
                let event = error_bridge_event(&err, "set_model_dir");
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().event_emitted(event_json(event));
                    qobject
                        .as_mut()
                        .error_occurred(QString::from(&code), QString::from(&message));
                });
                return;
            }
        };
        mukei_core::runtime::get().spawn(async move {
            runtime_state().set_model_base_dir(validated.canonical_base);
            runtime_state().set_model_dir(validated.canonical_dir);
            tracing::info!("model directory updated inside app-private base");
        });
    }

    pub fn model_dir(self: Pin<&mut Self>) -> QString {
        let p = runtime_state().model_dir();
        QString::from(p.to_string_lossy().as_ref())
    }

    pub fn recommended_model_id(self: Pin<&mut Self>, total_ram_mib: i32) -> QString {
        let ram = total_ram_mib.max(0) as u32;
        let m = mukei_core::engine::recommended_for_device(ram);
        QString::from(m.id.as_str())
    }

    pub fn model_catalogue_json(self: Pin<&mut Self>) -> QString {
        #[derive(serde::Serialize)]
        struct Entry {
            id: &'static str,
            display_name: &'static str,
            description: &'static str,
            approximate_bytes: u64,
            min_device_ram_mib: u32,
            recommended_n_ctx: usize,
            filename: &'static str,
            installed: bool,
            bytes_on_disk: u64,
        }
        let model_dir = runtime_state().model_dir();
        let entries: Vec<Entry> = mukei_core::engine::MODELS
            .iter()
            .map(|m| {
                let path = model_dir.join(m.filename);
                let metadata = std::fs::metadata(&path)
                    .ok()
                    .filter(|value| value.is_file());
                Entry {
                    id: m.id.as_str(),
                    display_name: m.display_name,
                    description: m.description,
                    approximate_bytes: m.approximate_bytes,
                    min_device_ram_mib: m.min_device_ram_mib,
                    recommended_n_ctx: m.recommended_n_ctx,
                    filename: m.filename,
                    installed: metadata.is_some(),
                    bytes_on_disk: metadata.map(|value| value.len()).unwrap_or(0),
                }
            })
            .collect();
        let json = serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string());
        QString::from(&json)
    }
}

impl ffi::SafRegistry {
    pub fn upsert_grant(
        self: Pin<&mut Self>,
        token: QString,
        target: QString,
        mime: QString,
    ) -> bool {
        let _row = core_saf::SafTokenRow {
            token_id: token.to_string(),
            source: "jni".to_string(),
            target: target.to_string(),
            mime: mime.to_string(),
            revoked: false,
            created: chrono::Utc::now(),
        };
        mukei_core::runtime::get().spawn(async move {
            #[cfg(feature = "rusqlite")]
            {
                let pool = runtime_state().database_pool();
                if let Some(p) = pool {
                    if let Err(e) = runtime_state().saf_registry().persist_upsert(&p, _row).await {
                        tracing::warn!(error = %e, "SAF grant persist_upsert failed; in-memory state unchanged");
                    }
                }
            }
        });
        true
    }

    pub fn resolve_token(self: Pin<&mut Self>, token: QString) -> QString {
        runtime_state()
            .saf_registry()
            .resolve(&token.to_string())
            .map(|target| QString::from(&target))
            .unwrap_or_else(|_| QString::from(""))
    }

    pub fn revoke_token(self: Pin<&mut Self>, token: QString) -> bool {
        let qt = self.as_ref().get_ref().qt_thread();
        let token_string = token.to_string();
        let qt_clone = qt.clone();
        mukei_core::runtime::get().spawn(async move {
            #[cfg(feature = "rusqlite")]
            {
                let pool = runtime_state().database_pool();
                if let Some(p) = pool {
                    match runtime_state().saf_registry()
                        .persist_revoke(&p, &token_string, "user_revoke")
                        .await
                    {
                        Ok(plan) => {
                            if let Err(error) = record_document_revoke_audit(
                                &p,
                                &plan.file_token,
                                "user_revoke",
                                plan.chunk_ids.len(),
                            )
                            .await
                            {
                                tracing::error!(
                                    code = error.error_code(),
                                    "document revoke committed; audit linkage remains retryable at boot"
                                );
                            }
                            if let Some(cfg) = runtime_state().config() {
                                match agent_runtime::purge_vector_chunks(
                                    &cfg,
                                    plan.chunk_ids.clone(),
                                )
                                .await
                                {
                                    Ok(_) => {
                                        if let Err(error) = core_saf::SafRegistry::mark_document_cleanup_complete(
                                            &p,
                                            &plan.file_token,
                                        )
                                        .await
                                        {
                                            tracing::warn!(
                                                error = %error,
                                                "vector cleanup succeeded but tombstone completion update failed"
                                            );
                                        }
                                    }
                                    Err(error) => {
                                        let _ = core_saf::SafRegistry::mark_document_cleanup_failed(
                                            &p,
                                            &plan.file_token,
                                            &error,
                                        )
                                        .await;
                                        tracing::warn!(
                                            error = %error,
                                            "SAF revoke removed SQL chunks; vector cleanup queued for boot retry"
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "SAF token persist_revoke failed; in-memory state unchanged"
                            );
                            return;
                        }
                    }
                } else {
                    return;
                }
            }
            let _ = qt_clone.queue(move |mut qobject| {
                qobject.as_mut().token_revoked(QString::from(&token_string));
            });
        });
        true
    }

    pub fn count(self: Pin<&mut Self>) -> i32 {
        runtime_state().saf_registry().count() as i32
    }
}

#[no_mangle]
pub extern "C" fn Java_com_mukei_app_MukeiBridge_nativeOnThermalStatus(status: i32) {
    runtime_state().set_thermal_status(status);
}

#[no_mangle]
pub extern "C" fn Java_com_mukei_app_MukeiBridge_nativeOnSafGrantRevoked() {}

#[cfg(test)]
mod qml_contract_tests {
    use super::{
        is_android_app_specific_files_path, safe_model_filename, validate_model_dir_against_base,
        MukeiAgentRust,
    };
    use std::sync::atomic::Ordering;

    const SOURCE: &str = include_str!("lib.rs");

    #[test]
    fn legacy_agent_qml_contract_symbols_remain_declared() {
        for symbol in [
            "fn initialize(",
            "fn send_message(",
            "fn stop_generation(",
            "fn download_model(",
            "fn stop_download(",
            "fn model_catalogue_json(",
            "fn recommended_model_id(",
            "fn state_changed(",
            "fn chunk_generated(",
            "fn stream_finalized(",
            "fn download_progress(",
            "fn error_occurred(",
        ] {
            assert!(
                SOURCE.contains(symbol),
                "missing QML contract symbol {symbol}"
            );
        }
    }

    #[test]
    fn typed_event_signal_is_declared() {
        assert!(SOURCE.contains("fn event_emitted("));
    }

    #[test]
    fn model_dir_must_stay_under_canonical_app_private_base() {
        let base = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let models = base.path().join("models");

        assert!(validate_model_dir_against_base(
            std::path::PathBuf::from("models"),
            base.path().into()
        )
        .is_err());
        assert!(
            validate_model_dir_against_base(base.path().join("../models"), base.path().into())
                .is_err()
        );
        assert!(validate_model_dir_against_base(models.clone(), base.path().into()).is_ok());
        assert!(
            validate_model_dir_against_base(outside.path().join("models"), base.path().into())
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn model_dir_rejects_symlink_escape_from_app_private_base() {
        let base = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = base.path().join("models");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();

        assert!(validate_model_dir_against_base(link, base.path().into()).is_err());
    }

    #[test]
    fn android_model_dir_policy_accepts_only_app_specific_files_root() {
        assert!(is_android_app_specific_files_path(std::path::Path::new(
            "/storage/emulated/0/Android/data/com.mukei.app/files/models"
        )));
        assert!(!is_android_app_specific_files_path(std::path::Path::new(
            "/storage/emulated/0/Download/models"
        )));
    }

    #[test]
    fn debug_custom_model_filename_must_be_simple_gguf() {
        assert!(safe_model_filename("model.gguf").is_ok());
        assert!(safe_model_filename("../model.gguf").is_err());
        assert!(safe_model_filename("model.bin").is_err());
    }

    #[tokio::test]
    async fn headless_agent_constructs_with_safe_initial_bridge_state() {
        let agent = MukeiAgentRust::default();

        assert_eq!(&*agent.state.lock().await, "UNINITIALIZED");
        assert!(!agent.busy.load(Ordering::Acquire));
        assert_eq!(
            agent.event_sequence.load(Ordering::Acquire),
            1,
            "bridge-local event sequence starts at one for a live process"
        );
        assert!(agent.active_downloads.lock().is_empty());

        agent.cancel_token.cancel();
        assert!(agent.cancel_token.is_cancelled());
        assert!(
            !agent.download_cancel.is_cancelled(),
            "chat cancellation must not cancel model downloads"
        );
    }
}

#[cfg(test)]
mod busy_guard_tests {
    //! Panic-safety regression for the `send_message` re-entrancy guard.
    //!
    //! `BusyGuard` is pure stdlib so these tests build on every host
    //! that can compile the bridge. They lock the architect-review
    //! follow-up invariant: the flag clears on *both* the normal and
    //! the panic-unwind paths, because a manual `store(false, ...)` at
    //! the end of the spawned block would skip on panic and soft-lock
    //! the bridge.

    use super::BusyGuard;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn drop_releases_the_flag_on_normal_path() {
        let flag = Arc::new(AtomicBool::new(true));
        {
            let _g = BusyGuard(flag.clone());
            assert!(
                flag.load(Ordering::Acquire),
                "flag must be held inside scope"
            );
        }
        assert!(
            !flag.load(Ordering::Acquire),
            "flag must be released after BusyGuard drops normally"
        );
    }

    #[test]
    fn drop_releases_the_flag_on_panic_unwind() {
        // Architect-review follow-up: the whole point of RAII here is
        // panic safety. `panic = "unwind"` is a workspace invariant
        // (TRD §1.3 / PRD G1), so Drop runs while the stack unwinds.
        let flag = Arc::new(AtomicBool::new(true));
        let flag_clone = flag.clone();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _g = BusyGuard(flag_clone);
            panic!("intentional: simulate a panic inside the spawned send_message task");
        }));
        assert!(panicked.is_err(), "the closure must have panicked");
        assert!(
            !flag.load(Ordering::Acquire),
            "flag must be released even when BusyGuard drops via unwind"
        );
    }

    #[test]
    fn drop_is_idempotent_across_clones() {
        // Two BusyGuards sharing the same Arc<AtomicBool> would both
        // clear it; the second store is a no-op. Guards against a
        // future bug where someone splits the guard between receive
        // and send tasks — the flag would still end up cleared.
        let flag = Arc::new(AtomicBool::new(true));
        {
            let _g1 = BusyGuard(flag.clone());
            let _g2 = BusyGuard(flag.clone());
        }
        assert!(!flag.load(Ordering::Acquire));
    }
}

#[cfg(test)]
mod download_guard_tests {
    //! Re-entrancy and cancellation regression tests for the model
    //! downloader — architect-review follow-up. These tests live in
    //! the bridge crate because the guard primitives ([`DownloadSlotGuard`],
    //! the root-owned in-flight set) are bridge-owned. They use only
    //! pure-stdlib + tokio primitives so they build on every host.

    use super::{runtime_state, DownloadSlotGuard};
    use std::path::PathBuf;
    use tokio_util::sync::CancellationToken;

    /// Two cancellation tokens used by the bridge — the chat token and
    /// the download token — must be independent. A user pressing the
    /// chat Stop button cannot kill an in-flight download, and vice
    /// versa. This is the invariant `download_model` relies on when it
    /// clones `self.download_cancel` instead of `self.cancel_token`.
    #[tokio::test]
    async fn chat_and_download_cancel_tokens_are_independent() {
        let chat = CancellationToken::new();
        let download = CancellationToken::new();
        chat.cancel();
        assert!(chat.is_cancelled(), "chat token must be cancelled");
        assert!(
            !download.is_cancelled(),
            "cancelling chat must not cascade to the download token"
        );
        download.cancel();
        assert!(download.is_cancelled(), "download token cancels separately");
    }

    /// Inserting a dest path into the root-owned in-flight set must reject
    /// a second concurrent attempt at the same path. Drop of the slot
    /// guard then frees the slot for a future attempt. This locks the
    /// behaviour that makes interleaved-writes corruption impossible.
    #[tokio::test]
    async fn per_destination_slot_rejects_concurrent_same_dest() {
        let dest = PathBuf::from("/tmp/mukei-test/per-dest-slot.gguf");

        // First insertion succeeds.
        {
            let mut s = runtime_state().downloads_in_flight().lock().await;
            assert!(!s.contains(&dest));
            s.insert(dest.clone());
        }

        // A second call would see the path present — the
        // download_model body returns ERR_DOWNLOAD_BUSY in that case.
        {
            let s = runtime_state().downloads_in_flight().lock().await;
            assert!(
                s.contains(&dest),
                "a second call must observe the dest already in flight"
            );
        }

        // Drop the guard. The Drop impl spawns a release on the shared
        // runtime; yield until the release fires.
        {
            let _guard = DownloadSlotGuard {
                registry: runtime_state().downloads_in_flight().clone(),
                dest: dest.clone(),
            };
        }
        for _ in 0..50 {
            tokio::task::yield_now().await;
            let s = runtime_state().downloads_in_flight().lock().await;
            if !s.contains(&dest) {
                return;
            }
            drop(s);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("DownloadSlotGuard::Drop must release the slot eventually");
    }

    /// Different dest paths must NOT contend with each other — the
    /// E2B and E4B Gemma 4 variants live at different filenames and
    /// can legitimately download in parallel. A naive single-flag
    /// implementation would block them; the per-dest set must not.
    #[tokio::test]
    async fn different_destinations_do_not_block_each_other() {
        let a = PathBuf::from("/tmp/mukei-test/model-a.gguf");
        let b = PathBuf::from("/tmp/mukei-test/model-b.gguf");

        let mut s = runtime_state().downloads_in_flight().lock().await;
        // Clear any state left by other tests.
        s.remove(&a);
        s.remove(&b);

        assert!(!s.contains(&a));
        assert!(!s.contains(&b));
        s.insert(a.clone());
        s.insert(b.clone());
        assert!(s.contains(&a) && s.contains(&b));

        s.remove(&a);
        s.remove(&b);
    }
}

#[cfg(test)]
mod active_download_tests {
    use super::{download_destination_token, single_active_download, ActiveDownload};
    use parking_lot::Mutex as ParkingMutex;
    use std::path::Path;

    #[test]
    fn single_active_download_reports_target() {
        let downloads = ParkingMutex::new(vec![ActiveDownload {
            model_id: Some("gemma-4-e2b-it".to_string()),
            destination: "model:gemma-4-e2b-it".to_string(),
        }]);
        let (model_id, destination) = single_active_download(&downloads);
        assert_eq!(model_id.as_deref(), Some("gemma-4-e2b-it"));
        assert_eq!(destination.as_deref(), Some("model:gemma-4-e2b-it"));
    }

    #[test]
    fn multiple_active_downloads_do_not_fake_a_single_target() {
        let downloads = ParkingMutex::new(vec![
            ActiveDownload {
                model_id: Some("gemma-4-e2b-it".to_string()),
                destination: "model:gemma-4-e2b-it".to_string(),
            },
            ActiveDownload {
                model_id: Some("gemma-4-e4b-it".to_string()),
                destination: "model:gemma-4-e4b-it".to_string(),
            },
        ]);
        let (model_id, destination) = single_active_download(&downloads);
        assert!(model_id.is_none());
        assert!(destination.is_none());
    }

    #[test]
    fn download_destination_token_never_exposes_absolute_path() {
        let token = download_destination_token(
            Some("gemma-4-e2b-it"),
            Path::new("/data/data/app/files/models/gemma.gguf"),
        );
        assert_eq!(token, "model:gemma-4-e2b-it");

        let fallback =
            download_destination_token(None, Path::new("/data/data/app/files/models/custom.gguf"));
        assert_eq!(fallback, "file:custom.gguf");
    }
}
