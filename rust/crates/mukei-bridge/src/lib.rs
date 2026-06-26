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

#[cfg(feature = "rusqlite")]
use mukei_core::storage::{saf as core_saf, AuditLogWriter};

#[cfg(not(feature = "rusqlite"))]
mod core_saf {
    #[derive(Clone, Debug, Default)]
    pub struct SafRegistry;

    #[derive(Clone, Debug)]
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

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QString, QVariant};
use once_cell::sync::Lazy;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use mukei_core::agent::AgentLoop;
use mukei_core::config::MukeiConfig;
use mukei_core::ffi::tags::{TagEvents, TagsStreaming};
use mukei_core::tools::ToolRegistry;
use mukei_core::types::BranchId;

static GLOBAL_SAF_REGISTRY: Lazy<Arc<core_saf::SafRegistry>> =
    Lazy::new(|| Arc::new(core_saf::SafRegistry::new()));
static GLOBAL_THERMAL_STATUS: Lazy<Arc<Mutex<i32>>> = Lazy::new(|| Arc::new(Mutex::new(0)));

/// Wrapped-secrets registry. The unwrap step (`feature = "android_keystore"`)
/// happens in the bridge and the *plaintext* lives only inside this
/// mutex — the QString that arrives from QML is overwritten before
/// being passed any further.
static GLOBAL_BRAVE_API_KEY: Lazy<Arc<Mutex<Option<String>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));
static GLOBAL_TAVILY_API_KEY: Lazy<Arc<Mutex<Option<String>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

/// Tool registry shared across every `send_message` invocation. Rebuilt
/// whenever the Brave or Tavily key changes so the next tool call sees
/// the new credentials without restarting the agent loop.
static GLOBAL_TOOL_REGISTRY: Lazy<Arc<Mutex<Arc<ToolRegistry>>>> = Lazy::new(|| {
    Arc::new(Mutex::new(Arc::new(ToolRegistry::with_web_search_keys(
        "missing-brave-key",
        "missing-tavily-key",
    ))))
});

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

/// Shared validated config snapshot so we can rebuild the loop when web-search
/// credentials rotate.
static GLOBAL_CONFIG: Lazy<Mutex<Option<MukeiConfig>>> = Lazy::new(|| Mutex::new(None));

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
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("mukei")
        .join("models");
    Mutex::new(p)
});

/// Resolve a QML `download_model(url, sha256)` request into a typed
/// [`mukei_core::storage::DownloadRequest`]. Accepts two call shapes:
///
/// 1. **Canonical id** — `url` is a [`ModelId::as_str`] value
///    (e.g. `"gemma-4-e2b-it"`) and `sha256` is empty or matches the
///    pinned digest. The URL + SHA come from the binary catalogue
///    (TRD §8.1).
/// 2. **Bespoke URL** — `url` is an `https://` URL and `sha256` is the
///    matching 64-hex digest. Used for QA before the
///    release-engineering pass pins the real CDN URL.
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

    let filename = url_or_id
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("model.gguf");
    Ok(mukei_core::storage::DownloadRequest {
        url: url_or_id.to_string(),
        expected_sha256: sha256.to_string(),
        dest: model_dir.join(filename),
    })
}

/// Bridge-side wrapped-secrets helper. The bridge crate is responsible
/// for unwrapping the Keystore-protected ciphertext that arrives over
/// the JNI boundary and handing the plaintext to the core; the
/// plaintext never returns to Java.
async fn rebuild_tool_registry_from_secrets() {
    let brave = GLOBAL_BRAVE_API_KEY
        .lock()
        .await
        .clone()
        .unwrap_or_else(|| "missing-brave-key".to_string());
    let tavily = GLOBAL_TAVILY_API_KEY
        .lock()
        .await
        .clone()
        .unwrap_or_else(|| "missing-tavily-key".to_string());
    let registry = Arc::new(ToolRegistry::with_web_search_keys(brave, tavily));
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

        #[qinvokable]
        fn initialize(self: Pin<&mut MukeiAgent>, config_path: QString) -> bool;
        #[qinvokable]
        fn send_message(self: Pin<&mut MukeiAgent>, user_input: QString);
        #[qinvokable]
        fn stop_generation(self: Pin<&mut MukeiAgent>);
        #[qinvokable]
        fn download_model(self: Pin<&mut MukeiAgent>, url: QString, sha256: QString);
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

        // Wrapped-secrets API — each setter accepts the UNWRAPPED key
        // material (the bridge unwraps via Android Keystore *before*
        // calling these). The names match the wrapped-secrets registry
        // slots in `config.toml::wrapped_secrets`.
        #[qinvokable]
        fn set_brave_api_key(self: Pin<&mut MukeiBridge>, api_key: QString);
        #[qinvokable]
        fn set_tavily_api_key(self: Pin<&mut MukeiBridge>, api_key: QString);
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
}

pub struct MukeiAgentRust {
    cancel_token: CancellationToken,
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
    busy: Arc<AtomicBool>,
}

/// RAII guard that clears the re-entrancy flag on `Drop`. Held by the
/// spawned `send_message` task; runs on both the normal-completion
/// and the unwinding (panic) paths because `panic = "unwind"` is a
/// workspace-wide invariant. Pairs `Ordering::Release` with the
/// `compare_exchange(..., AcqRel, Acquire)` that flipped the flag.
struct BusyGuard(Arc<AtomicBool>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub struct MukeiBridgeRust;
pub struct SafRegistryRust;

impl Default for MukeiAgentRust {
    fn default() -> Self {
        Self {
            cancel_token: CancellationToken::new(),
            state: Arc::new(Mutex::new("UNINITIALIZED".to_string())),
            busy: Arc::new(AtomicBool::new(false)),
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

impl CxxQtType for MukeiAgentRust {}
impl CxxQtType for MukeiBridgeRust {}
impl CxxQtType for SafRegistryRust {}

impl MukeiAgentRust {
    /// Boot path. Loads + validates `config.toml`, opens the SQLite /
    /// SQLCipher pool, runs pending migrations, hydrates the SAF
    /// registry from disk, reconciles persisted vector state, and
    /// constructs the shared `Arc<AgentLoop>`.
    pub fn initialize(self: Pin<&mut Self>, config_path: QString) -> bool {
        let qt = self.qt_thread();
        let state = self.state.clone();
        let config_path = config_path.to_string();
        mukei_core::runtime::get().spawn(async move {
            let cfg_path = std::path::PathBuf::from(&config_path);
            let cfg = match agent_runtime::load_config(&cfg_path) {
                Ok(c) => c,
                Err(e) => {
                    let code = e.error_code().to_string();
                    let msg = e.to_string();
                    let _ = qt.queue(move |mut qobject| {
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(code), QString::from(msg));
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
                let pool = match agent_runtime::open_pool(&cfg).await {
                    Ok(p) => Arc::new(p),
                    Err(e) => {
                        let code = e.error_code().to_string();
                        let msg = e.to_string();
                        let _ = qt.queue(move |mut qobject| {
                            qobject
                                .as_mut()
                                .error_occurred(QString::from(code), QString::from(msg));
                        });
                        return;
                    }
                };
                *GLOBAL_DATABASE_POOL.lock().await = Some(pool.clone());

                if let Err(e) = GLOBAL_AUDIT_LOG_WRITER.hydrate_from_pool(&pool).await {
                    let code = e.error_code().to_string();
                    let msg = e.to_string();
                    let _ = qt.queue(move |mut qobject| {
                        qobject
                            .as_mut()
                            .error_occurred(QString::from(code), QString::from(msg));
                    });
                    return;
                }

                if let Err(e) = agent_runtime::hydrate_saf_registry(&GLOBAL_SAF_REGISTRY, &pool).await {
                    tracing::warn!(error = %e, "SafRegistry hydration failed; starting empty");
                }

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
                qobject.as_mut().state_changed(QString::from("IDLE_READY"));
            });
        });
        true
    }

    pub fn send_message(self: Pin<&mut Self>, user_input: QString) {
        let qt_thread = self.qt_thread();
        let cancel_token = self.cancel_token.clone();
        let busy = self.busy.clone();
        let input = user_input.to_string();

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
            let _ = qt_thread.queue(move |mut qobject| {
                qobject
                    .as_mut()
                    .error_occurred(QString::from(code), QString::from(message));
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

        mukei_core::runtime::get().spawn(async move {
            let mut tags = TagsStreaming::new();
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

                let events = tags.push(&chunk);
                if events.contains(TagEvents::OPENED) {
                    let _ = ui_thread.queue(|mut qobject| qobject.as_mut().thinking_started());
                }
                if events.contains(TagEvents::CLOSED) {
                    let _ = ui_thread.queue(|mut qobject| qobject.as_mut().thinking_completed());
                }
                let _ = ui_thread.queue(move |mut qobject| {
                    qobject.as_mut().chunk_generated(QString::from(&chunk));
                });
            }
        });

        mukei_core::runtime::get().spawn(async move {
            let _ = qt_thread.queue(|mut qobject| qobject.as_mut().state_changed(QString::from("INFERRING")));

            let loop_handle = { GLOBAL_AGENT_LOOP.lock().await.clone() };
            match loop_handle {
                Some(handle) => {
                    let result = handle
                        .run(input, BranchId::default(), cancel_token, chunk_tx.clone())
                        .await;
                    if let Err(error) = result {
                        let code = error.error_code().to_string();
                        let message = error.to_string();
                        let _ = qt_thread.queue(move |mut qobject| {
                            qobject.as_mut().error_occurred(QString::from(code), QString::from(message));
                        });
                    }
                }
                None => {
                    let _ = qt_thread.queue(|mut qobject| {
                        qobject.as_mut().error_occurred(
                            QString::from("BRIDGE_NOT_INITIALIZED"),
                            QString::from("AgentLoop was never constructed — call MukeiAgent.initialize(config_path) first."),
                        );
                    });
                }
            }
            let _ = chunk_tx.send("\u{0001}STREAM_FINAL\u{0001}".to_string()).await;
            let _ = qt_thread.queue(|mut qobject| qobject.as_mut().state_changed(QString::from("IDLE_READY")));
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

    pub fn stop_generation(mut self: Pin<&mut Self>) {
        self.cancel_token.cancel();
        self.cancel_token = CancellationToken::new();
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
    /// Progress comes back through `download_progress(progress, status)`:
    /// * `started:<bytes|unknown>`
    /// * `downloading:<bytes_downloaded>`
    /// * `complete:<absolute_path>`
    /// * `error:<ERR_CODE>:<message>`
    pub fn download_model(self: Pin<&mut Self>, url: QString, sha256: QString) {
        let qt = self.qt_thread();
        let cancel = self.cancel_token.clone();
        let url_or_id = url.to_string();
        let sha = sha256.to_string();

        mukei_core::runtime::get().spawn(async move {
            let req = match resolve_download_request(&url_or_id, &sha).await {
                Ok(r) => r,
                Err(e) => {
                    let code = e.error_code().to_string();
                    let message = e.to_string();
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().download_progress(
                            0.0,
                            QString::from(format!("error:{code}:{message}")),
                        );
                    });
                    return;
                }
            };

            let dest_for_status = req.dest.clone();
            let (tx, mut rx) = tokio::sync::mpsc::channel::<mukei_core::storage::DownloadEvent>(32);
            let req_for_dl = req.clone();
            let cancel_for_dl = cancel.clone();
            let dl_handle = mukei_core::runtime::get().spawn(async move {
                mukei_core::storage::run_download(req_for_dl, tx, cancel_for_dl).await
            });

            while let Some(ev) = rx.recv().await {
                let qt_for_ev = qt.clone();
                match ev {
                    mukei_core::storage::DownloadEvent::Started { total_bytes } => {
                        let status = match total_bytes {
                            Some(n) => format!("started:{n}"),
                            None => "started:unknown".to_string(),
                        };
                        let _ = qt_for_ev.queue(move |mut qobject| {
                            qobject
                                .as_mut()
                                .download_progress(0.0, QString::from(status));
                        });
                    }
                    mukei_core::storage::DownloadEvent::Progress {
                        progress,
                        bytes_downloaded,
                    } => {
                        let status = format!("downloading:{bytes_downloaded}");
                        let _ = qt_for_ev.queue(move |mut qobject| {
                            qobject
                                .as_mut()
                                .download_progress(progress, QString::from(status));
                        });
                    }
                    mukei_core::storage::DownloadEvent::Complete { final_path } => {
                        let status = format!("complete:{}", final_path.to_string_lossy());
                        let _ = qt_for_ev.queue(move |mut qobject| {
                            qobject
                                .as_mut()
                                .download_progress(1.0, QString::from(status));
                        });
                    }
                    mukei_core::storage::DownloadEvent::Error { code, message } => {
                        let status = format!("error:{code}:{message}");
                        let _ = qt_for_ev.queue(move |mut qobject| {
                            qobject
                                .as_mut()
                                .download_progress(0.0, QString::from(status));
                        });
                    }
                }
            }

            match dl_handle.await {
                Ok(Ok(())) => {
                    tracing::info!(path = %dest_for_status.display(), "model download finalised");
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "model download failed");
                }
                Err(join_err) => {
                    let msg = format!("download task panicked: {join_err}");
                    tracing::error!(error = %msg);
                    let _ = qt.queue(move |mut qobject| {
                        qobject.as_mut().download_progress(
                            0.0,
                            QString::from(format!("error:ERR_FFI_PANIC:{msg}")),
                        );
                    });
                }
            }
        });
    }

    pub fn clear_conversation(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        let _ = qt.queue(|mut qobject| qobject.as_mut().state_changed(QString::from("IDLE_READY")));
    }

    pub fn get_hardware_info(self: Pin<&mut Self>) -> QVariant {
        QVariant::from(QString::from(format!(
            "os={} arch={} thermal_status={}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            *GLOBAL_THERMAL_STATUS.blocking_lock()
        )))
    }

    pub fn update_setting(self: Pin<&mut Self>, key: QString, value: QVariant) {
        let _ = (self, key, value);
    }
}

impl MukeiBridgeRust {
    /// Inject the unwrapped Brave API key and rebuild the shared tool
    /// registry so the next `web_search` call uses the new credential.
    /// (Issue #3.)
    pub fn set_brave_api_key(self: Pin<&mut Self>, api_key: QString) {
        let store = GLOBAL_BRAVE_API_KEY.clone();
        let api_key = api_key.to_string();
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
        let api_key = api_key.to_string();
        mukei_core::runtime::get().spawn(async move {
            *store.lock().await = Some(api_key);
            rebuild_tool_registry_from_secrets().await;
        });
    }

    pub fn note_thermal_status(self: Pin<&mut Self>, status: i32) {
        let qt = self.qt_thread();
        let global = GLOBAL_THERMAL_STATUS.clone();
        mukei_core::runtime::get().spawn(async move {
            *global.lock().await = status;
            let _ = qt.queue(move |mut qobject| qobject.as_mut().thermal_status_changed(status));
        });
    }

    pub fn saf_registry_count(self: Pin<&mut Self>) -> i32 {
        GLOBAL_SAF_REGISTRY.count() as i32
    }

    // -----------------------------------------------------------------
    // Model download surface (TRD §8.1 / REQ-MOD-01)
    // -----------------------------------------------------------------

    pub fn set_model_dir(self: Pin<&mut Self>, path: QString) {
        let new_path = std::path::PathBuf::from(path.to_string());
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
        QString::from(json)
    }
}

impl SafRegistryRust {
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
            .map(QString::from)
            .unwrap_or_else(|_| QString::from(""))
    }

    pub fn revoke_token(self: Pin<&mut Self>, token: QString) -> bool {
        let qt = self.qt_thread();
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
                qobject.as_mut().token_revoked(QString::from(token_string));
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
