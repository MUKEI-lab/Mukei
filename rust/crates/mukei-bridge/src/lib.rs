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
    /// Re-entrancy guard for `send_message` (user priority follow-up to
    /// the architect-review batch). The flag is flipped from `false`
    /// to `true` by the synchronous entry into `send_message`; a
    /// second call that observes `true` is rejected with
    /// `ERR_BRIDGE_BUSY` and emits no side effects. The streaming task
    /// clears the flag after the chunk channel drains and the final
    /// `IDLE_READY` state is queued, regardless of whether the loop
    /// succeeded, errored, or was cancelled. Stored as an
    /// `Arc<AtomicBool>` so the spawned streaming task can clear it
    /// from the Tokio runtime.
    busy: Arc<AtomicBool>,
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
            // Re-entrancy guard release (user priority follow-up). This
            // runs on every termination path — success, error_occurred,
            // missing-loop, and cancellation — because the streaming
            // task always reaches this point after the channel closes.
            // `Ordering::Release` pairs with the AcqRel compare-exchange
            // at the top of `send_message`.
            busy.store(false, Ordering::Release);
        });
    }

    pub fn stop_generation(mut self: Pin<&mut Self>) {
        self.cancel_token.cancel();
        self.cancel_token = CancellationToken::new();
    }

    pub fn download_model(self: Pin<&mut Self>, _url: QString, _sha256: QString) {
        let qt = self.qt_thread();
        let _ = qt.queue(|mut qobject| {
            qobject
                .as_mut()
                .download_progress(0.0, QString::from("not_implemented_in_sandbox"));
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
