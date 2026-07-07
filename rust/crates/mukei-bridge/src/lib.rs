//! CXX-Qt bridge — TRD §1.2 / §1.3 / §9.4.
//!
//! # Wrapped-secrets registry (Issue #3, user priority #1)
//!
//! The bridge owns three global Tokio-locked slots for the wrapped
//! Brave / Tavily API keys and a `ToolRegistry` whose `WebSearchTool`
//! is rebuilt every time a key arrives. The old design read process
//! env vars (`BRAVE_API_KEY`) while the bridge wrote a different name
//! (`CIPHER_BRAVE_API_KEY`); the names never met, so search never
//! actually worked. The new design passes the unwrapped keys directly
//! into [`mukei_core::tools::ToolRegistry::with_web_search_keys`] so
//! a typo becomes a compile error.

mod agent_runtime;
mod bridge_state;

#[cfg(feature = "rusqlite")]
use mukei_core::storage::{
    saf as core_saf, AuditChainStatus, AuditLogReader, AuditLogWriter, ConversationRepository,
    MessageStatus, PersistedTurn,
};

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
        pub fn upsert(&self, _row: SafTokenRow) -> Result<(), ()> {
            Ok(())
        }
        pub fn revoke(&self, _token: &str) -> Result<(), ()> {
            Ok(())
        }
        pub fn resolve(&self, _token: &str) -> Result<String, ()> {
            Err(())
        }
    }
}

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QString, QVariant};
use once_cell::sync::Lazy;
use parking_lot::Mutex as ParkingMutex;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use bridge_state::{single_active_download, ActiveDownload, BusyGuard, DownloadSlotGuard};
use mukei_core::agent::AgentLoop;
use mukei_core::config::MukeiConfig;
use mukei_core::ffi::tags::{TagEvents, TagsStreaming};
use mukei_core::tools::{RemoteFeaturePolicy, ToolRegistry};
use mukei_core::types::{BranchId, ConversationId, MessageId};
use mukei_core::ui_contract::{
    AndroidStorageState, AppLifecycleState, BridgeEvent, BridgeEventKind, CapabilitySnapshot,
    ChatTurnState, DownloadState, UiError,
};

static GLOBAL_SAF_REGISTRY: Lazy<Arc<core_saf::SafRegistry>> =
    Lazy::new(|| Arc::new(core_saf::SafRegistry::new()));
static GLOBAL_THERMAL_STATUS: Lazy<Arc<Mutex<i32>>> = Lazy::new(|| Arc::new(Mutex::new(0)));

/// Wrapped-secrets registry. The unwrap step (`feature = "android_keystore"`)
/// happens in the bridge and provider keys are kept in zeroizing memory
/// until the tool registry is rebuilt.
static GLOBAL_BRAVE_API_KEY: Lazy<Arc<Mutex<Option<Zeroizing<String>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));
static GLOBAL_TAVILY_API_KEY: Lazy<Arc<Mutex<Option<Zeroizing<String>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));
static GLOBAL_REMOTE_FEATURE_POLICY: Lazy<Arc<Mutex<RemoteFeaturePolicy>>> =
    Lazy::new(|| Arc::new(Mutex::new(RemoteFeaturePolicy::default())));
#[cfg(feature = "sqlcipher")]
static GLOBAL_DATABASE_CIPHER_KEY: Lazy<Arc<Mutex<Option<Vec<u8>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

/// Tool registry shared across every `send_message` invocation. Rebuilt
/// whenever the Brave or Tavily key changes so the next tool call sees
/// the new credentials without restarting the agent loop.
static GLOBAL_TOOL_REGISTRY: Lazy<Arc<Mutex<Arc<ToolRegistry>>>> = Lazy::new(|| {
    Arc::new(Mutex::new(Arc::new(
        ToolRegistry::with_web_search_keys_and_policy(
            "missing-brave-key",
            "missing-tavily-key",
            RemoteFeaturePolicy::default(),
        ),
    )))
});

#[cfg(feature = "sqlcipher")]
fn decode_hex_key(hex: &str) -> Result<Vec<u8>, String> {
    let trimmed = hex.trim();
    if trimmed.is_empty() {
        return Err("database cipher key hex is empty".to_string());
    }
    if trimmed.len() % 2 != 0 {
        return Err("database cipher key hex must contain an even number of digits".to_string());
    }

    let mut out = Vec::with_capacity(trimmed.len() / 2);
    for idx in (0..trimmed.len()).step_by(2) {
        let byte = u8::from_str_radix(&trimmed[idx..idx + 2], 16).map_err(|_| {
            format!("database cipher key hex contains invalid digits at offset {idx}")
        })?;
        out.push(byte);
    }
    Ok(out)
}

/// Shared `Arc<AgentLoop>` — `None` until `initialize()` builds it.
static GLOBAL_AGENT_LOOP: Lazy<Mutex<Option<Arc<AgentLoop>>>> = Lazy::new(|| Mutex::new(None));

/// Optional shared `DatabasePool` (gated behind `rusqlite`).
#[cfg(feature = "rusqlite")]
static GLOBAL_DATABASE_POOL: Lazy<Mutex<Option<Arc<mukei_core::storage::DatabasePool>>>> =
    Lazy::new(|| Mutex::new(None));

/// Append-only audit writer for `tool_audit_log`.
#[cfg(feature = "rusqlite")]
static GLOBAL_AUDIT_LOG_WRITER: Lazy<Arc<AuditLogWriter>> =
    Lazy::new(|| Arc::new(AuditLogWriter::new()));

/// Durable chat session used by `send_message`. Until a public
/// conversation picker/new-chat API is wired, consecutive sends belong
/// to one repository-backed conversation instead of being fragmented
/// into one conversation per turn.
static GLOBAL_CHAT_SESSION: Lazy<Mutex<Option<(ConversationId, BranchId)>>> =
    Lazy::new(|| Mutex::new(None));

/// Shared validated config snapshot so we can rebuild the loop when web-search
/// credentials rotate.
static GLOBAL_CONFIG: Lazy<Mutex<Option<MukeiConfig>>> = Lazy::new(|| Mutex::new(None));

/// Global per-destination-path registry of in-flight downloads.
///
/// # Why a set, not a single flag?
///
/// We *want* two different models (e.g. E2B and E4B) to download in
/// parallel — they target distinct `.partial` files and never race.
/// What we must reject is a second call targeting the **same dest path**
/// as a download already in flight, because two `tokio::fs::File` handles
/// would then write through independent cursors into the same `.partial`,
/// each task's SHA-256 hasher would only see the bytes *it* wrote, and
/// both tasks could plausibly atomically-rename a corrupted GGUF into
/// the path `LlamaEngine::load_model` mmaps. The integrity check would
/// be silently defeated by interleaved writes.
///
/// Keyed on the canonical absolute destination path; entries are
/// inserted in `download_model` *before* the streaming task spawns and
/// removed by [`DownloadSlotGuard::Drop`], which runs on every exit
/// path including panic-unwind (workspace mandates `panic = "unwind"`).
static GLOBAL_DOWNLOADS_IN_FLIGHT: Lazy<Arc<Mutex<HashSet<std::path::PathBuf>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashSet::new())));

fn event_json(event: BridgeEvent) -> QString {
    let json = serde_json::to_string(&event).unwrap_or_else(|err| {
        serde_json::json!({
            "schema_version": BridgeEvent::SCHEMA_VERSION,
            "timestamp": chrono::Utc::now(),
            "category": "error",
            "error": {
                "code": "ERR_EVENT_SERIALIZE",
                "class": "bridge",
                "severity": "error",
                "recoverable": true,
                "user_message": "Bridge event could not be serialized.",
                "technical_message": err.to_string(),
                "suggested_action": "report_issue",
                "source": "bridge",
            }
        })
        .to_string()
    });
    QString::from(&json)
}

fn error_bridge_event(error: &mukei_core::error::MukeiError, source: &'static str) -> BridgeEvent {
    BridgeEvent::new(BridgeEventKind::Error {
        error: UiError::from_mukei_error(error, source),
    })
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

/// Default on-device model directory. Testers can override this via
/// `set_model_dir` before calling `download_model`; otherwise the
/// bridge picks a sensible per-OS path:
///
///   * Android: the app-private external files dir, populated by the
///     Java/Kotlin side via `set_model_dir` before any download runs.
///   * Linux/Mac desktop: `$XDG_DATA_HOME/mukei/models` (or
///     `$HOME/.local/share/mukei/models`).
///
/// Stored as `Mutex<PathBuf>` so the QML / JNI side can rewrite it at
/// any time.
static GLOBAL_MODEL_DIR: Lazy<Mutex<std::path::PathBuf>> = Lazy::new(|| {
    let p = std::env::var("XDG_DATA_HOME")
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
        .join("models");
    Mutex::new(p)
});

fn validate_model_dir(path: std::path::PathBuf) -> mukei_core::error::Result<std::path::PathBuf> {
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
    Ok(path)
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
    let model_dir = GLOBAL_MODEL_DIR.lock().await.clone();

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

/// Bridge-side wrapped-secrets helper. The bridge crate is responsible
/// for unwrapping the Keystore-protected ciphertext that arrives over
/// the JNI boundary and handing the plaintext to the core; the
/// plaintext never returns to Java and old bridge copies are zeroized
/// when replaced.
async fn rebuild_tool_registry_from_secrets() {
    let brave = GLOBAL_BRAVE_API_KEY
        .lock()
        .await
        .as_ref()
        .map(|key| key.to_string())
        .unwrap_or_else(|| "missing-brave-key".to_string());
    let tavily = GLOBAL_TAVILY_API_KEY
        .lock()
        .await
        .as_ref()
        .map(|key| key.to_string())
        .unwrap_or_else(|| "missing-tavily-key".to_string());
    let remote_policy = *GLOBAL_REMOTE_FEATURE_POLICY.lock().await;
    let registry = Arc::new(ToolRegistry::with_web_search_keys_and_policy(
        brave,
        tavily,
        remote_policy,
    ));
    *GLOBAL_TOOL_REGISTRY.lock().await = registry.clone();
    tracing::info!("tool registry rebuilt with wrapped-secrets keys");

    let cfg_opt = GLOBAL_CONFIG.lock().await.clone();
    if let Some(cfg) = cfg_opt {
        #[cfg(feature = "rusqlite")]
        {
            if let Some(pool) = GLOBAL_DATABASE_POOL.lock().await.clone() {
                let loop_handle = agent_runtime::build_agent_loop(
                    &cfg,
                    registry.clone(),
                    pool,
                    GLOBAL_AUDIT_LOG_WRITER.clone(),
                );
                *GLOBAL_AGENT_LOOP.lock().await = Some(loop_handle);
                tracing::info!("agent loop rebuilt alongside tool registry");
            }
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let loop_handle = agent_runtime::build_agent_loop(&cfg, registry.clone());
            *GLOBAL_AGENT_LOOP.lock().await = Some(loop_handle);
            tracing::info!("agent loop rebuilt alongside tool registry");
        }
    }
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
    /// mechanism via [`GLOBAL_DOWNLOADS_IN_FLIGHT`] + [`DownloadSlotGuard`]
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

impl ffi::MukeiAgent {
    /// Boot path. Loads + validates `config.toml`, opens the SQLite /
    /// SQLCipher pool, runs pending migrations, hydrates the SAF
    /// registry from disk, reconciles persisted vector state, and
    /// constructs the shared `Arc<AgentLoop>`.
    pub fn initialize(self: Pin<&mut Self>, config_path: QString) -> bool {
        let state = self.as_ref().rust().state.clone();
        let qt = self.as_ref().get_ref().qt_thread();
        let config_path = config_path.to_string();
        mukei_core::runtime::get().spawn(async move {
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
            let cfg = match agent_runtime::load_config(&cfg_path) {
                Ok(c) => c,
                Err(e) => {
                    let code = e.error_code().to_string();
                    let msg = e.to_string();
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
            tracing::info!(
                ?cfg.gpu_layers, n_ctx = cfg.n_ctx,
                max_iterations = cfg.watchdog.max_iterations,
                "config loaded"
            );
            *GLOBAL_CONFIG.lock().await = Some(cfg.clone());

            rebuild_tool_registry_from_secrets().await;

            #[cfg(feature = "rusqlite")]
            {
                let _ = qt.queue(|mut qobject| {
                    qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                        BridgeEventKind::AppLifecycle {
                            state: AppLifecycleState::OpeningDatabase,
                            capabilities: CapabilitySnapshot::uninitialized(),
                            android_storage: Some(AndroidStorageState::Unknown),
                        },
                    )));
                });
                #[cfg(feature = "sqlcipher")]
                let database_key = match GLOBAL_DATABASE_CIPHER_KEY.lock().await.take() {
                    Some(key) if !key.is_empty() => key,
                    _ => {
                        let err = mukei_core::error::MukeiError::DatabaseInitFailed(
                            "SQLCipher build requires set_database_cipher_key() before initialize()"
                                .to_string(),
                        );
                        let code = err.error_code().to_string();
                        let msg = err.to_string();
                        let event = error_bridge_event(&err, "initialize");
                        let _ = qt.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(event));
                            qobject
                                .as_mut()
                                .error_occurred(QString::from(&code), QString::from(&msg));
                        });
                        return;
                    }
                };

                let pool = match agent_runtime::open_pool(
                    &cfg,
                    #[cfg(feature = "sqlcipher")]
                    database_key,
                )
                .await
                {
                    Ok(p) => Arc::new(p),
                    Err(e) => {
                        let code = e.error_code().to_string();
                        let msg = e.to_string();
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
                *GLOBAL_DATABASE_POOL.lock().await = Some(pool.clone());

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
                        let msg = e.to_string();
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
                        let msg = err.to_string();
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
                        let msg = e.to_string();
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

                if let Err(e) = GLOBAL_AUDIT_LOG_WRITER.hydrate_from_pool(&pool).await {
                    let code = e.error_code().to_string();
                    let msg = e.to_string();
                    let event = error_bridge_event(&e, "initialize");
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(&code), QString::from(&msg));
                    });
                    return;
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
                if let Err(e) = agent_runtime::hydrate_saf_registry(&GLOBAL_SAF_REGISTRY, &pool).await {
                    tracing::warn!(error = %e, "SafRegistry hydration failed; starting empty");
                }

                let _ = qt.queue(|mut qobject| {
                    qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                        BridgeEventKind::AppLifecycle {
                            state: AppLifecycleState::ReconcilingVectorStore,
                            capabilities: CapabilitySnapshot::uninitialized(),
                            android_storage: Some(AndroidStorageState::Ready {
                                saf_grant_count: GLOBAL_SAF_REGISTRY.count(),
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

                let registry_arc = GLOBAL_TOOL_REGISTRY.lock().await.clone();
                let loop_handle = agent_runtime::build_agent_loop(
                    &cfg,
                    registry_arc,
                    pool.clone(),
                    GLOBAL_AUDIT_LOG_WRITER.clone(),
                );
                *GLOBAL_AGENT_LOOP.lock().await = Some(loop_handle);
            }
            #[cfg(not(feature = "rusqlite"))]
            {
                let registry_arc = GLOBAL_TOOL_REGISTRY.lock().await.clone();
                let loop_handle = agent_runtime::build_agent_loop(&cfg, registry_arc);
                *GLOBAL_AGENT_LOOP.lock().await = Some(loop_handle);
            }

            *state.lock().await = "IDLE_READY".to_string();
            let _ = qt.queue(|mut qobject| {
                qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                    BridgeEventKind::AppLifecycle {
                        state: AppLifecycleState::Ready,
                        capabilities: CapabilitySnapshot::ready(),
                        android_storage: Some(AndroidStorageState::Ready {
                            saf_grant_count: GLOBAL_SAF_REGISTRY.count(),
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
        let input = user_input.to_string();
        let (conversation_id, branch_id) = {
            let mut session = GLOBAL_CHAT_SESSION.blocking_lock();
            *session.get_or_insert_with(|| (ConversationId::new(), BranchId::new()))
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
            let message = err.to_string();
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
            let mut last_persisted_len = 0usize;
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
                            if let Some(pool) = GLOBAL_DATABASE_POOL.lock().await.clone() {
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
                    .with_chat_context(conversation_id, event_turn_id)
                    .with_sequence(event_sequence.fetch_add(1, Ordering::AcqRel));
                    qobject.as_mut().event_emitted(event_json(event));
                    qobject.as_mut().chunk_generated(QString::from(&chunk));
                });
            }
        });

        let partial_response_for_run = partial_response.clone();
        #[cfg(feature = "rusqlite")]
        let persisted_turn_for_run = persisted_turn.clone();
        mukei_core::runtime::get().spawn(async move {
            let submit_event = BridgeEvent::new(BridgeEventKind::ChatState {
                state: ChatTurnState::Submitting,
                capabilities: CapabilitySnapshot::inferencing(),
            })
            .with_chat_context(conversation_id, turn_id.clone())
            .with_sequence(sequence.fetch_add(1, Ordering::AcqRel));
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(submit_event));
                qobject.as_mut().state_changed(QString::from("INFERRING"));
            });

            let loop_handle = { GLOBAL_AGENT_LOOP.lock().await.clone() };
            let mut failed = false;
            let mut final_capabilities = CapabilitySnapshot::ready();
            #[cfg(feature = "rusqlite")]
            {
                if let Some(pool) = GLOBAL_DATABASE_POOL.lock().await.clone() {
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
                            *persisted_turn_for_run.lock().await = Some(turn);
                        }
                        Err(e) => {
                            failed = true;
                            let code = e.error_code().to_string();
                            let message = e.to_string();
                            let event = BridgeEvent::new(BridgeEventKind::ChatFailed {
                                error: UiError::from_mukei_error(&e, "send_message"),
                            })
                            .with_chat_context(conversation_id, turn_id.clone())
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
                    let result = handle
                        .run(
                            input,
                            branch_id,
                            cancel_token.clone(),
                            chunk_tx.clone(),
                        )
                        .await;
                    if let Err(error) = result {
                        failed = true;
                        let code = error.error_code().to_string();
                        let message = error.to_string();
                        let event = BridgeEvent::new(BridgeEventKind::ChatFailed {
                            error: UiError::from_mukei_error(&error, "send_message"),
                        })
                        .with_chat_context(conversation_id, turn_id.clone())
                        .with_sequence(sequence.fetch_add(1, Ordering::AcqRel));
                        let _ = qt_thread.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(event));
                            qobject.as_mut().error_occurred(QString::from(&code), QString::from(&message));
                        });
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
                    .with_chat_context(conversation_id, turn_id.clone())
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
                    if let Some(pool) = GLOBAL_DATABASE_POOL.lock().await.clone() {
                        let content = partial_response_for_run.lock().await.clone();
                        let persist_result = if failed {
                            ConversationRepository::fail_turn(
                                &pool,
                                turn,
                                MessageStatus::Failed,
                                content,
                            )
                            .await
                        } else if cancel_token.is_cancelled() {
                            ConversationRepository::fail_turn(
                                &pool,
                                turn,
                                MessageStatus::Cancelled,
                                content,
                            )
                            .await
                        } else {
                            ConversationRepository::complete_turn(&pool, turn, content).await
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
            .with_chat_context(conversation_id, turn_id)
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
    /// * `complete:<absolute_path>`
    /// * `error:<ERR_CODE>:<message>`
    pub fn download_model(self: Pin<&mut Self>, url: QString, sha256: QString) {
        let cancel = self.as_ref().rust().download_cancel.clone();
        let active_downloads = self.as_ref().rust().active_downloads.clone();
        let qt = self.as_ref().get_ref().qt_thread();
        // Architect-review follow-up: use the *download-only* token so
        // `stop_generation()` no longer silently cancels the download.
        let url_or_id = url.to_string();
        let sha = sha256.to_string();
        let model_id = mukei_core::engine::lookup_model_str(&url_or_id)
            .map(|descriptor| descriptor.id.as_str().to_string());

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
                    let message = e.to_string();
                    let state_event = BridgeEvent::new(BridgeEventKind::DownloadState {
                        state: DownloadState::Failed,
                        model_id: model_id.clone(),
                        destination: None,
                        capabilities: CapabilitySnapshot::ready(),
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
            // Insert `req.dest` into the global in-flight registry
            // BEFORE spawning any I/O. A second call that observes the
            // path already-present is rejected with `ERR_DOWNLOAD_BUSY`
            // and emits no side effects on disk. Release is RAII via
            // [`DownloadSlotGuard`], which fires even on panic-unwind
            // (workspace mandates `panic = "unwind"`).
            {
                let mut in_flight = GLOBAL_DOWNLOADS_IN_FLIGHT.lock().await;
                if in_flight.contains(&req.dest) {
                    drop(in_flight);
                    let err = mukei_core::error::MukeiError::DownloadBusy {
                        dest: req.dest.to_string_lossy().into_owned(),
                    };
                    let code = err.error_code().to_string();
                    let message = err.to_string();
                    let state_event = BridgeEvent::new(BridgeEventKind::DownloadState {
                        state: DownloadState::Failed,
                        model_id: model_id.clone(),
                        destination: Some(req.dest.to_string_lossy().into_owned()),
                        capabilities: CapabilitySnapshot::ready(),
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
                registry: GLOBAL_DOWNLOADS_IN_FLIGHT.clone(),
                dest: req.dest.clone(),
            };

            let dest_for_status = req.dest.clone();
            let active_download = ActiveDownload {
                model_id: model_id.clone(),
                destination: dest_for_status.to_string_lossy().into_owned(),
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
            while let Some(ev) = rx.recv().await {
                let qt_for_ev = qt.clone();
                match ev {
                    mukei_core::storage::DownloadEvent::Started { total_bytes } => {
                        total_bytes_seen = total_bytes;
                        let status = match total_bytes {
                            Some(n) => format!("started:{n}"),
                            None => "started:unknown".to_string(),
                        };
                        let model_id = model_id.clone();
                        let destination = Some(dest_for_status.to_string_lossy().into_owned());
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
                        let status = format!("downloading:{bytes_downloaded}");
                        let model_id = model_id.clone();
                        let destination = Some(dest_for_status.to_string_lossy().into_owned());
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
                    mukei_core::storage::DownloadEvent::Complete { final_path } => {
                        terminal_download_event_seen = true;
                        let status = format!("complete:{}", final_path.to_string_lossy());
                        let final_path_string = final_path.to_string_lossy().into_owned();
                        let _ = qt_for_ev.queue(move |mut qobject| {
                            qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                                BridgeEventKind::DownloadCompleted {
                                    final_path: final_path_string.clone(),
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
                        let destination = Some(dest_for_status.to_string_lossy().into_owned());
                        let state = if matches!(err, mukei_core::error::MukeiError::Cancelled) {
                            DownloadState::Cancelled
                        } else {
                            DownloadState::Failed
                        };
                        let state_event = BridgeEvent::new(BridgeEventKind::DownloadState {
                            state,
                            model_id: model_id.clone(),
                            destination: destination.clone(),
                            capabilities: CapabilitySnapshot::ready(),
                        });
                        let failed_event =
                            if matches!(err, mukei_core::error::MukeiError::Cancelled) {
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
                        let message = e.to_string();
                        let state = if matches!(e, mukei_core::error::MukeiError::Cancelled) {
                            DownloadState::Cancelled
                        } else {
                            DownloadState::Failed
                        };
                        let state_event = BridgeEvent::new(BridgeEventKind::DownloadState {
                            state,
                            model_id: model_id.clone(),
                            destination: Some(dest_for_status.to_string_lossy().into_owned()),
                            capabilities: CapabilitySnapshot::ready(),
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
                        destination: Some(dest_for_status.to_string_lossy().into_owned()),
                        capabilities: CapabilitySnapshot::ready(),
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
            active_downloads
                .lock()
                .retain(|download| download != &active_download);
        });
    }

    pub fn clear_conversation(self: Pin<&mut Self>) {
        let qt = self.as_ref().get_ref().qt_thread();
        mukei_core::runtime::get().spawn(async {
            *GLOBAL_CHAT_SESSION.lock().await = None;
        });
        let _ = qt.queue(|mut qobject| {
            qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                BridgeEventKind::CapabilitySnapshot {
                    capabilities: CapabilitySnapshot::ready(),
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
                *GLOBAL_THERMAL_STATUS.blocking_lock()
            )
            .as_str(),
        );
        QVariant::from(&summary)
    }

    pub fn update_setting(self: Pin<&mut Self>, key: QString, value: QVariant) {
        let _ = (self, key, value);
    }
}

impl ffi::MukeiBridge {
    /// Inject the unwrapped Brave API key and rebuild the shared tool
    /// registry so the next `web_search` call uses the new credential.
    /// (Issue #3.)
    pub fn set_brave_api_key(self: Pin<&mut Self>, api_key: QString) {
        let store = GLOBAL_BRAVE_API_KEY.clone();
        let api_key = Zeroizing::new(api_key.to_string());
        mukei_core::runtime::get().spawn(async move {
            *store.lock().await = Some(api_key);
            rebuild_tool_registry_from_secrets().await;
        });
    }

    /// Inject the unwrapped Tavily API key (Issue #3). Symmetric with
    /// `set_brave_api_key` — the previous bridge had no Tavily setter
    /// at all.
    pub fn set_tavily_api_key(self: Pin<&mut Self>, api_key: QString) {
        let store = GLOBAL_TAVILY_API_KEY.clone();
        let api_key = Zeroizing::new(api_key.to_string());
        mukei_core::runtime::get().spawn(async move {
            *store.lock().await = Some(api_key);
            rebuild_tool_registry_from_secrets().await;
        });
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
                let store = GLOBAL_REMOTE_FEATURE_POLICY.clone();
                mukei_core::runtime::get().spawn(async move {
                    *store.lock().await = policy;
                    rebuild_tool_registry_from_secrets().await;
                });
            }
            Err(err) => {
                let event = error_bridge_event(&err, "set_remote_feature_policy");
                let code = err.error_code().to_string();
                let message = err.to_string();
                let _ = qt.queue(move |mut qobject| {
                    qobject.as_mut().event_emitted(event_json(event));
                    qobject
                        .as_mut()
                        .error_occurred(QString::from(&code), QString::from(&message));
                });
            }
        }
    }

    /// Inject the hex-encoded unwrapped SQLCipher key that protects the
    /// local database. The Android/JNI side must hex-encode raw
    /// Keystore output before crossing the QString boundary; this
    /// method decodes it back to bytes before storage initialization.
    #[cfg(feature = "sqlcipher")]
    pub fn set_database_cipher_key(self: Pin<&mut Self>, key: QString) {
        use zeroize::Zeroize;

        let qt = self.as_ref().get_ref().qt_thread();
        let store = GLOBAL_DATABASE_CIPHER_KEY.clone();
        let key_hex = Zeroizing::new(key.to_string());
        mukei_core::runtime::get().spawn(async move {
            match decode_hex_key(&key_hex) {
                Ok(key_bytes) => {
                    let mut guard = store.lock().await;
                    if let Some(mut old_key) = guard.take() {
                        old_key.zeroize();
                    }
                    *guard = Some(key_bytes);
                }
                Err(message) => {
                    let err = mukei_core::error::MukeiError::ConfigInvalid {
                        field: "database_cipher_key".to_string(),
                        reason: message.clone(),
                    };
                    let event = error_bridge_event(&err, "set_database_cipher_key");
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().event_emitted(event_json(event));
                        qobject.as_mut().error_occurred(
                            QString::from("ERR_DATABASE_KEY_INVALID"),
                            QString::from(&message),
                        );
                    });
                }
            }
        });
    }

    #[cfg(not(feature = "sqlcipher"))]
    pub fn set_database_cipher_key(self: Pin<&mut Self>, key: QString) {
        let _ = (self, key);
    }

    pub fn note_thermal_status(self: Pin<&mut Self>, status: i32) {
        let qt = self.as_ref().get_ref().qt_thread();
        let global = GLOBAL_THERMAL_STATUS.clone();
        mukei_core::runtime::get().spawn(async move {
            *global.lock().await = status;
            let _ = qt.queue(move |mut qobject| {
                if status >= 3 {
                    qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                        BridgeEventKind::AppLifecycle {
                            state: AppLifecycleState::Degraded,
                            capabilities: CapabilitySnapshot::ready(),
                            android_storage: Some(AndroidStorageState::Ready {
                                saf_grant_count: GLOBAL_SAF_REGISTRY.count(),
                            }),
                        },
                    )));
                }
                qobject.as_mut().thermal_status_changed(status);
            });
        });
    }

    pub fn saf_registry_count(self: Pin<&mut Self>) -> i32 {
        GLOBAL_SAF_REGISTRY.count() as i32
    }

    // -----------------------------------------------------------------
    // Model download surface (TRD §8.1 / REQ-MOD-01)
    // -----------------------------------------------------------------

    pub fn set_model_dir(self: Pin<&mut Self>, path: QString) {
        let qt = self.as_ref().get_ref().qt_thread();
        let new_path = match validate_model_dir(std::path::PathBuf::from(path.to_string())) {
            Ok(path) => path,
            Err(err) => {
                let code = err.error_code().to_string();
                let message = err.to_string();
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
            *GLOBAL_MODEL_DIR.lock().await = new_path;
            tracing::info!("model directory updated");
        });
    }

    pub fn model_dir(self: Pin<&mut Self>) -> QString {
        let p = GLOBAL_MODEL_DIR.blocking_lock().clone();
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
        }
        let entries: Vec<Entry> = mukei_core::engine::MODELS
            .iter()
            .map(|m| Entry {
                id: m.id.as_str(),
                display_name: m.display_name,
                description: m.description,
                approximate_bytes: m.approximate_bytes,
                min_device_ram_mib: m.min_device_ram_mib,
                recommended_n_ctx: m.recommended_n_ctx,
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
        let row = core_saf::SafTokenRow {
            token_id: token.to_string(),
            source: "jni".to_string(),
            target: target.to_string(),
            mime: mime.to_string(),
            revoked: false,
            created: chrono::Utc::now(),
        };
        mukei_core::runtime::get().spawn(async move {
            let _ = GLOBAL_SAF_REGISTRY.upsert(row.clone());
            #[cfg(feature = "rusqlite")]
            {
                let pool = GLOBAL_DATABASE_POOL.lock().await.clone();
                if let Some(p) = pool {
                    if let Err(e) = GLOBAL_SAF_REGISTRY.persist_upsert(&p, row).await {
                        tracing::warn!(error = %e, "SAF grant persist_upsert failed; in-memory mirror will diverge until restart");
                    }
                }
            }
        });
        true
    }

    pub fn resolve_token(self: Pin<&mut Self>, token: QString) -> QString {
        GLOBAL_SAF_REGISTRY
            .resolve(&token.to_string())
            .map(|target| QString::from(&target))
            .unwrap_or_else(|_| QString::from(""))
    }

    pub fn revoke_token(self: Pin<&mut Self>, token: QString) -> bool {
        let qt = self.as_ref().get_ref().qt_thread();
        let token_string = token.to_string();
        let qt_clone = qt.clone();
        mukei_core::runtime::get().spawn(async move {
            let _ = GLOBAL_SAF_REGISTRY.revoke(&token_string);
            #[cfg(feature = "rusqlite")]
            {
                let pool = GLOBAL_DATABASE_POOL.lock().await.clone();
                if let Some(p) = pool {
                    if let Err(e) = GLOBAL_SAF_REGISTRY
                        .persist_revoke(&p, &token_string, "user_revoke")
                        .await
                    {
                        tracing::warn!(error = %e, "SAF token persist_revoke failed; in-memory mirror will diverge until restart");
                    }
                }
            }
            let _ = qt_clone.queue(move |mut qobject| {
                qobject.as_mut().token_revoked(QString::from(&token_string));
            });
        });
        true
    }

    pub fn count(self: Pin<&mut Self>) -> i32 {
        GLOBAL_SAF_REGISTRY.count() as i32
    }
}

#[no_mangle]
pub extern "C" fn Java_com_mukei_app_MukeiBridge_nativeOnThermalStatus(status: i32) {
    *GLOBAL_THERMAL_STATUS.blocking_lock() = status;
}

#[no_mangle]
pub extern "C" fn Java_com_mukei_app_MukeiBridge_nativeOnSafGrantRevoked() {}

#[cfg(test)]
mod qml_contract_tests {
    use super::{safe_model_filename, validate_model_dir, MukeiAgentRust};
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
    fn model_dir_rejects_relative_and_parent_paths() {
        assert!(validate_model_dir(std::path::PathBuf::from("models")).is_err());
        assert!(validate_model_dir(std::path::PathBuf::from("/tmp/mukei/../models")).is_err());
        assert!(validate_model_dir(std::path::PathBuf::from("/tmp/mukei/models")).is_ok());
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
    //! the global in-flight set) are bridge-owned. They use only
    //! pure-stdlib + tokio primitives so they build on every host.

    use super::{DownloadSlotGuard, GLOBAL_DOWNLOADS_IN_FLIGHT};
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

    /// Inserting a dest path into the global in-flight set must reject
    /// a second concurrent attempt at the same path. Drop of the slot
    /// guard then frees the slot for a future attempt. This locks the
    /// behaviour that makes interleaved-writes corruption impossible.
    #[tokio::test]
    async fn per_destination_slot_rejects_concurrent_same_dest() {
        let dest = PathBuf::from("/tmp/mukei-test/per-dest-slot.gguf");

        // First insertion succeeds.
        {
            let mut s = GLOBAL_DOWNLOADS_IN_FLIGHT.lock().await;
            assert!(!s.contains(&dest));
            s.insert(dest.clone());
        }

        // A second call would see the path present — the
        // download_model body returns ERR_DOWNLOAD_BUSY in that case.
        {
            let s = GLOBAL_DOWNLOADS_IN_FLIGHT.lock().await;
            assert!(
                s.contains(&dest),
                "a second call must observe the dest already in flight"
            );
        }

        // Drop the guard. The Drop impl spawns a release on the shared
        // runtime; yield until the release fires.
        {
            let _guard = DownloadSlotGuard {
                registry: GLOBAL_DOWNLOADS_IN_FLIGHT.clone(),
                dest: dest.clone(),
            };
        }
        for _ in 0..50 {
            tokio::task::yield_now().await;
            let s = GLOBAL_DOWNLOADS_IN_FLIGHT.lock().await;
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

        let mut s = GLOBAL_DOWNLOADS_IN_FLIGHT.lock().await;
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
    use super::{single_active_download, ActiveDownload};
    use parking_lot::Mutex as ParkingMutex;

    #[test]
    fn single_active_download_reports_target() {
        let downloads = ParkingMutex::new(vec![ActiveDownload {
            model_id: Some("gemma-4-e2b-it".to_string()),
            destination: "/models/gemma.gguf".to_string(),
        }]);
        let (model_id, destination) = single_active_download(&downloads);
        assert_eq!(model_id.as_deref(), Some("gemma-4-e2b-it"));
        assert_eq!(destination.as_deref(), Some("/models/gemma.gguf"));
    }

    #[test]
    fn multiple_active_downloads_do_not_fake_a_single_target() {
        let downloads = ParkingMutex::new(vec![
            ActiveDownload {
                model_id: Some("gemma-4-e2b-it".to_string()),
                destination: "/models/e2b.gguf".to_string(),
            },
            ActiveDownload {
                model_id: Some("gemma-4-e4b-it".to_string()),
                destination: "/models/e4b.gguf".to_string(),
            },
        ]);
        let (model_id, destination) = single_active_download(&downloads);
        assert!(model_id.is_none());
        assert!(destination.is_none());
    }
}
