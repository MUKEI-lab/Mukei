//! CXX-Qt bridge — TRD §1.2 / §1.3 / §9.4.

use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QString, QVariant};
use once_cell::sync::Lazy;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use mukei_core::storage::saf as core_saf;

static GLOBAL_SAF_REGISTRY: Lazy<Arc<core_saf::SafRegistry>> =
    Lazy::new(|| Arc::new(core_saf::SafRegistry::new()));
static GLOBAL_THERMAL_STATUS: Lazy<Arc<Mutex<i32>>> =
    Lazy::new(|| Arc::new(Mutex::new(0)));
static GLOBAL_CIPHER_API_KEY: Lazy<Arc<Mutex<Option<String>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

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

        #[qinvokable]
        fn set_cipher_api_key(self: Pin<&mut MukeiBridge>, api_key: QString);
        #[qinvokable]
        fn note_thermal_status(self: Pin<&mut MukeiBridge>, status: i32);
        #[qinvokable]
        fn saf_registry_count(self: Pin<&mut MukeiBridge>) -> i32;

        #[qobject]
        pub type SafRegistry = super::SafRegistryRust;

        #[qsignal]
        fn token_revoked(self: Pin<&mut SafRegistry>, token: QString);

        #[qinvokable]
        fn upsert_grant(self: Pin<&mut SafRegistry>, token: QString, target: QString, mime: QString) -> bool;
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
}

pub struct MukeiBridgeRust;
pub struct SafRegistryRust;

impl Default for MukeiAgentRust {
    fn default() -> Self {
        Self {
            cancel_token: CancellationToken::new(),
            state: Arc::new(Mutex::new("UNINITIALIZED".to_string())),
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
    pub fn initialize(self: Pin<&mut Self>, _config_path: QString) -> bool {
        let qt = self.qt_thread();
        let state = self.state.clone();
        mukei_core::runtime::get().spawn(async move {
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
        let input = user_input.to_string();
        let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<String>(256);
        let ui_thread = qt_thread.clone();

        mukei_core::runtime::get().spawn(async move {
            while let Some(chunk) = chunk_rx.recv().await {
                if chunk == "\u{0001}STREAM_FINAL\u{0001}" {
                    let _ = ui_thread.queue(|mut qobject| qobject.as_mut().stream_finalized());
                    continue;
                }
                if chunk == "\u{0001}THINKING_STARTED\u{0001}" {
                    let _ = ui_thread.queue(|mut qobject| qobject.as_mut().thinking_started());
                    continue;
                }
                if chunk == "\u{0001}THINKING_COMPLETED\u{0001}" {
                    let _ = ui_thread.queue(|mut qobject| qobject.as_mut().thinking_completed());
                    continue;
                }
                let _ = ui_thread.queue(move |mut qobject| {
                    qobject.as_mut().chunk_generated(QString::from(&chunk));
                });
            }
        });

        mukei_core::runtime::get().spawn(async move {
            let _ = qt_thread.queue(|mut qobject| qobject.as_mut().state_changed(QString::from("INFERRING")));
            let _ = chunk_tx.send("\u{0001}THINKING_STARTED\u{0001}".to_string()).await;
            let result = mukei_core::engine::llama_wrapper::run_inference(&input, cancel_token, chunk_tx.clone()).await;
            let _ = chunk_tx.send("\u{0001}THINKING_COMPLETED\u{0001}".to_string()).await;
            if let Err(error) = result {
                let code = error.error_code().to_string();
                let message = error.to_string();
                let _ = qt_thread.queue(move |mut qobject| {
                    qobject.as_mut().error_occurred(QString::from(code), QString::from(message));
                });
            }
            let _ = chunk_tx.send("\u{0001}STREAM_FINAL\u{0001}".to_string()).await;
            let _ = qt_thread.queue(|mut qobject| qobject.as_mut().state_changed(QString::from("IDLE_READY")));
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
    pub fn set_cipher_api_key(self: Pin<&mut Self>, api_key: QString) {
        let store = GLOBAL_CIPHER_API_KEY.clone();
        let api_key = api_key.to_string();
        std::env::set_var("CIPHER_BRAVE_API_KEY", &api_key);
        mukei_core::runtime::get().spawn(async move {
            *store.lock().await = Some(api_key);
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
    pub fn upsert_grant(self: Pin<&mut Self>, token: QString, target: QString, mime: QString) -> bool {
        #[cfg(feature = "rusqlite")]
        {
            let row = core_saf::SafTokenRow {
                token_id: token.to_string(),
                source: "jni".to_string(),
                target: target.to_string(),
                mime: mime.to_string(),
                revoked: false,
                created: chrono::Utc::now(),
            };
            return GLOBAL_SAF_REGISTRY.upsert(row).is_ok();
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let _ = (token, target, mime);
            false
        }
    }

    pub fn resolve_token(self: Pin<&mut Self>, token: QString) -> QString {
        #[cfg(feature = "rusqlite")]
        {
            return GLOBAL_SAF_REGISTRY
                .resolve(&token.to_string())
                .map(QString::from)
                .unwrap_or_else(|_| QString::from(""));
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let _ = token;
            QString::from("")
        }
    }

    pub fn revoke_token(self: Pin<&mut Self>, token: QString) -> bool {
        #[cfg(feature = "rusqlite")]
        {
            let qt = self.qt_thread();
            let token_string = token.to_string();
            if GLOBAL_SAF_REGISTRY.revoke(&token_string).is_ok() {
                let _ = qt.queue(move |mut qobject| qobject.as_mut().token_revoked(QString::from(token_string)));
                return true;
            }
            return false;
        }
        #[cfg(not(feature = "rusqlite"))]
        {
            let _ = token;
            false
        }
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
