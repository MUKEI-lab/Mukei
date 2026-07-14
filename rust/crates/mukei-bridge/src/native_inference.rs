//! Production llama.cpp adapter over the Mukei-owned native capsule.
//!
//! The native ABI is intentionally small and provenance-checked. GGUF artifact
//! integrity is verified by the activation owner before this module can load a
//! model; this module owns only native construction and per-turn execution.

use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr::NonNull;
use std::sync::Arc;

use async_trait::async_trait;
use mukei_core::config::MukeiConfig;
use mukei_core::engine::{
    BackendIdentity, InferenceBackend, InferenceBackendFactory, InferenceOutcome, StopReason,
    VerifiedModelDescriptor,
};
use mukei_core::error::{MukeiError, Result};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const EXPECTED_ABI_VERSION: u32 = 1;
const EXPECTED_BUILD_ID: &str = "7c082bc417bbe53210a83df4ba5b49e18ce6193c";
const STATUS_OK: i32 = 0;
const STATUS_CANCELLED: i32 = 8;

#[repr(C)]
struct NativeModel {
    _private: [u8; 0],
}

type TokenCallback = Option<extern "C" fn(*const u8, usize, *mut c_void)>;
type CancelCallback = Option<extern "C" fn(*mut c_void) -> bool>;

unsafe extern "C" {
    fn mukei_llama_abi_version() -> u32;
    fn mukei_llama_build_id() -> *const c_char;
    fn mukei_llama_status_message(code: i32) -> *const c_char;
    fn mukei_llama_model_load(
        path: *const c_char,
        n_ctx: u32,
        n_threads: u32,
        gpu_layers: i32,
        out_model: *mut *mut NativeModel,
    ) -> i32;
    fn mukei_llama_model_free(model: *mut NativeModel);
    fn mukei_llama_generate(
        model: *mut NativeModel,
        prompt: *const u8,
        prompt_len: usize,
        max_new_tokens: u32,
        token_callback: TokenCallback,
        cancel_callback: CancelCallback,
        user_data: *mut c_void,
        out_generated_tokens: *mut u32,
    ) -> i32;
}

fn native_build_id() -> Option<String> {
    let ptr = unsafe { mukei_llama_build_id() };
    if ptr.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned(),
    )
}

pub(crate) fn implementation_available() -> bool {
    (unsafe { mukei_llama_abi_version() }) == EXPECTED_ABI_VERSION
        && native_build_id().as_deref() == Some(EXPECTED_BUILD_ID)
}

fn status_message(code: i32) -> String {
    let ptr = unsafe { mukei_llama_status_message(code) };
    if ptr.is_null() {
        return "native inference failed".to_string();
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

struct NativeModelHandle(NonNull<NativeModel>);

unsafe impl Send for NativeModelHandle {}
unsafe impl Sync for NativeModelHandle {}

impl Drop for NativeModelHandle {
    fn drop(&mut self) {
        unsafe { mukei_llama_model_free(self.0.as_ptr()) };
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NativeLlamaBackendFactory {
    n_ctx: u32,
    n_threads: u32,
    gpu_layers: i32,
    max_new_tokens: u32,
}

impl NativeLlamaBackendFactory {
    pub(crate) fn from_config(config: &MukeiConfig) -> Self {
        let max_new_tokens = config.watchdog.max_token_budget.clamp(1, u32::MAX as u64) as u32;
        Self {
            n_ctx: config.n_ctx.max(1),
            n_threads: config.n_threads.max(1),
            gpu_layers: config.gpu_layers,
            max_new_tokens,
        }
    }
}

struct NativeLlamaBackend {
    model: Arc<NativeModelHandle>,
    max_new_tokens: u32,
}

#[async_trait]
impl InferenceBackendFactory for NativeLlamaBackendFactory {
    async fn activate(
        &self,
        descriptor: &VerifiedModelDescriptor,
    ) -> Result<Arc<dyn InferenceBackend>> {
        if !implementation_available() {
            return Err(MukeiError::ModelLoadFailed(
                "native inference ABI or provenance does not match this build".to_string(),
            ));
        }
        let catalogue =
            mukei_core::engine::lookup_model_str(&descriptor.model_id).ok_or_else(|| {
                MukeiError::ModelLoadFailed(
                    "selected model is not in the trusted catalogue".to_string(),
                )
            })?;
        if descriptor.revision != catalogue.expected_sha256
            || descriptor.artifact.artifact_id() != catalogue.expected_sha256
        {
            return Err(MukeiError::ModelCorrupted);
        }
        let path = descriptor.artifact.local_path().to_path_buf();
        let n_ctx = self.n_ctx;
        let n_threads = self.n_threads;
        let gpu_layers = self.gpu_layers;
        let max_new_tokens = self.max_new_tokens;
        let handle = tokio::task::spawn_blocking(move || {
            let path = CString::new(path.to_string_lossy().as_bytes()).map_err(|_| {
                MukeiError::ModelLoadFailed("model path contains an invalid NUL byte".to_string())
            })?;
            let mut raw = std::ptr::null_mut();
            let status = unsafe {
                mukei_llama_model_load(path.as_ptr(), n_ctx, n_threads, gpu_layers, &mut raw)
            };
            if status != STATUS_OK {
                return Err(MukeiError::ModelLoadFailed(status_message(status)));
            }
            let model = NonNull::new(raw).ok_or_else(|| {
                MukeiError::ModelLoadFailed("native inference returned no model handle".to_string())
            })?;
            Ok::<_, MukeiError>(NativeModelHandle(model))
        })
        .await
        .map_err(|error| MukeiError::BlockingJoinFailed(error.to_string()))??;
        Ok(Arc::new(NativeLlamaBackend {
            model: Arc::new(handle),
            max_new_tokens,
        }))
    }
}

struct CallbackState {
    sender: mpsc::Sender<String>,
    cancel: CancellationToken,
    pending: Vec<u8>,
    assistant_text: String,
    callback_error: Option<String>,
}

impl CallbackState {
    fn send_text(&mut self, text: String) {
        if text.is_empty() || self.callback_error.is_some() {
            return;
        }
        self.assistant_text.push_str(&text);
        if self.sender.blocking_send(text).is_err() {
            self.callback_error =
                Some("stream receiver closed during native inference".to_string());
        }
    }

    fn drain_complete_utf8(&mut self) {
        loop {
            if self.pending.is_empty() || self.callback_error.is_some() {
                return;
            }
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    let owned = text.to_string();
                    self.pending.clear();
                    self.send_text(owned);
                    return;
                }
                Err(error) if error.valid_up_to() > 0 => {
                    let valid = error.valid_up_to();
                    let owned = String::from_utf8(self.pending[..valid].to_vec())
                        .expect("validated UTF-8 prefix");
                    self.pending.drain(..valid);
                    self.send_text(owned);
                }
                Err(error) if error.error_len().is_none() => return,
                Err(_) => {
                    self.callback_error = Some(
                        "native inference emitted an invalid UTF-8 token sequence".to_string(),
                    );
                    return;
                }
            }
        }
    }
}

extern "C" fn token_callback(data: *const u8, len: usize, user_data: *mut c_void) {
    if user_data.is_null() || (data.is_null() && len != 0) {
        return;
    }
    let state = unsafe { &mut *(user_data.cast::<CallbackState>()) };
    if len > 0 {
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        state.pending.extend_from_slice(bytes);
        state.drain_complete_utf8();
    }
}

extern "C" fn cancel_callback(user_data: *mut c_void) -> bool {
    if user_data.is_null() {
        return true;
    }
    let state = unsafe { &*(user_data.cast::<CallbackState>()) };
    state.cancel.is_cancelled() || state.callback_error.is_some()
}

#[async_trait]
impl InferenceBackend for NativeLlamaBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity::production("llama_cpp_native")
    }

    async fn run(
        &self,
        prompt: &str,
        cancel: CancellationToken,
        token_sender: mpsc::Sender<String>,
    ) -> Result<InferenceOutcome> {
        if prompt.is_empty() {
            return Err(MukeiError::Invariant("empty prompt".to_string()));
        }
        let model = self.model.clone();
        let prompt = prompt.as_bytes().to_vec();
        let max_new_tokens = self.max_new_tokens;
        let generation = tokio::task::spawn_blocking(move || {
            let mut state = CallbackState {
                sender: token_sender,
                cancel,
                pending: Vec::new(),
                assistant_text: String::new(),
                callback_error: None,
            };
            let mut generated_tokens = 0u32;
            let status = unsafe {
                mukei_llama_generate(
                    model.0.as_ptr(),
                    prompt.as_ptr(),
                    prompt.len(),
                    max_new_tokens,
                    Some(token_callback),
                    Some(cancel_callback),
                    (&mut state as *mut CallbackState).cast::<c_void>(),
                    &mut generated_tokens,
                )
            };
            state.drain_complete_utf8();
            if state.callback_error.is_none() && !state.pending.is_empty() {
                state.callback_error = Some(
                    "native inference ended with an incomplete UTF-8 token sequence".to_string(),
                );
            }
            (
                status,
                generated_tokens,
                state.assistant_text,
                state.callback_error,
            )
        })
        .await
        .map_err(|error| MukeiError::BlockingJoinFailed(error.to_string()))?;

        let (status, generated_tokens, assistant_text, callback_error) = generation;
        if let Some(error) = callback_error {
            return Err(MukeiError::Internal(error));
        }
        match status {
            STATUS_OK => Ok(InferenceOutcome {
                assistant_text,
                used_tokens: u64::from(generated_tokens),
                stop_reason: StopReason::Completed,
            }),
            STATUS_CANCELLED => Ok(InferenceOutcome {
                assistant_text,
                used_tokens: u64::from(generated_tokens),
                stop_reason: StopReason::UserStopped,
            }),
            other => Err(MukeiError::ModelLoadFailed(status_message(other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_capsule_identity_matches_pinned_contract() {
        assert!(implementation_available());
        assert_eq!(native_build_id().as_deref(), Some(EXPECTED_BUILD_ID));
    }

    #[test]
    fn split_utf8_token_pieces_are_coalesced_without_loss() {
        let (sender, mut receiver) = mpsc::channel(4);
        let mut state = CallbackState {
            sender,
            cancel: CancellationToken::new(),
            pending: Vec::new(),
            assistant_text: String::new(),
            callback_error: None,
        };
        state.pending.extend_from_slice(&[0xE2, 0x82]);
        state.drain_complete_utf8();
        assert!(receiver.try_recv().is_err());
        state.pending.push(0xAC);
        state.drain_complete_utf8();
        assert_eq!(receiver.try_recv().unwrap(), "€");
        assert_eq!(state.assistant_text, "€");
        assert!(state.callback_error.is_none());
    }
}
