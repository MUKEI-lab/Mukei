//! Stable UI-facing contract for the Rust-to-QML bridge.
//!
//! This module is intentionally JSON-first. Rust owns application truth,
//! the bridge owns Qt-safe delivery, and QML can render these snapshots
//! without parsing legacy raw strings such as `started:123` or `INFERRING`.

use serde::{Deserialize, Serialize};

use crate::error::MukeiError;
use crate::types::{ConversationId, MessageId};

/// Current high-level application lifecycle state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppLifecycleState {
    /// No boot attempt has run yet.
    Uninitialized,
    /// Bridge boot has been requested.
    Booting,
    /// A valid config is required before startup can continue.
    NeedsConfig,
    /// A database key is required before encrypted storage can open.
    NeedsDatabaseKey,
    /// The config file is being loaded and validated.
    LoadingConfig,
    /// SQLite or SQLCipher storage is being opened.
    OpeningDatabase,
    /// Pending migrations are being applied.
    ApplyingMigrations,
    /// Persisted Android SAF grants are being loaded.
    HydratingSaf,
    /// Persisted SQL rows and vector-store contents are being compared.
    ReconcilingVectorStore,
    /// Model state is being loaded or verified.
    LoadingModel,
    /// App is ready for normal interaction.
    Ready,
    /// App is usable with known reduced capability.
    Degraded,
    /// Startup reached an unrecoverable error.
    FatalError,
}

/// Current state of one chat turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatTurnState {
    /// No active turn.
    Idle,
    /// User input has been accepted by the bridge.
    Submitting,
    /// The model is preparing a response.
    Thinking,
    /// Assistant tokens are streaming.
    Streaming,
    /// A tool call is executing.
    ToolCalling,
    /// Cancellation has been requested.
    Cancelling,
    /// The turn was cancelled.
    Cancelled,
    /// The turn completed normally.
    Completed,
    /// The turn failed.
    Failed,
}

/// Current model-download lifecycle state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    /// No active download.
    Idle,
    /// Download request has been accepted but not started.
    Queued,
    /// Download task is starting.
    Starting,
    /// Bytes are being transferred.
    Downloading,
    /// The final artifact is being verified.
    Verifying,
    /// Download completed and the artifact is verified.
    Completed,
    /// Cancellation has been requested.
    Cancelling,
    /// Download was cancelled.
    Cancelled,
    /// Download failed.
    Failed,
}

/// Android storage and SAF visibility for the UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AndroidStorageState {
    /// Platform storage status is not known yet.
    Unknown,
    /// Storage is currently unavailable.
    Unavailable {
        /// Stable reason string suitable for routing, not localization.
        reason: String,
    },
    /// A SAF picker or persisted grant is required before the action can run.
    SafPermissionRequired,
    /// Persisted SAF grant state is loading.
    HydratingSaf,
    /// Storage and SAF state are usable.
    Ready {
        /// Number of known non-revoked SAF grants.
        saf_grant_count: usize,
    },
}

/// Severity of an error as presented to UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorSeverity {
    /// Informational diagnostic.
    Info,
    /// User-visible warning with a likely recovery path.
    Warning,
    /// User-visible operation failure.
    Error,
    /// Fatal condition that blocks normal app operation.
    Fatal,
}

/// Suggested next UI action for an error.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestedUiAction {
    /// No special action.
    None,
    /// Retry the failed operation.
    Retry,
    /// Open app settings.
    OpenSettings,
    /// Open model manager.
    OpenModelManager,
    /// Request Android storage or SAF permission.
    RequestStoragePermission,
    /// Stop the active generation.
    StopGeneration,
    /// Clear the failed download state.
    ClearDownload,
    /// Report a bug with diagnostics.
    ReportIssue,
    /// Restart the app.
    RestartApp,
}

/// UI-friendly error payload that preserves the stable core error code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiError {
    /// Stable `ERR_*` code from [`MukeiError::error_code`].
    pub code: String,
    /// Stable lower-case error class from [`MukeiError::classification`].
    pub class: String,
    /// UI severity.
    pub severity: ErrorSeverity,
    /// Whether the user or UI can reasonably recover.
    pub recoverable: bool,
    /// Human-readable user-facing message.
    pub user_message: String,
    /// Technical message preserving core detail.
    pub technical_message: String,
    /// Suggested next UI action.
    pub suggested_action: SuggestedUiAction,
    /// Subsystem that produced the error.
    pub source: String,
}

impl UiError {
    /// Build a UI error from an existing core error and bridge/core source label.
    ///
    /// Security: `technical_message` is the only field that the UI may
    /// surface to advanced / debug panels — and it is the field most
    /// likely to embed a raw filesystem path, an Authorization header,
    /// an `api_key=...` token, or a prompt fragment when an upstream
    /// crate's error formatting leaks it. We therefore funnel
    /// `error.to_string()` through the diagnostics redactor before it
    /// crosses the bridge boundary. `sanitize_error_message` uses
    /// cheap structural checks (looks_like_secret / looks_like_path /
    /// redact_inline_secrets) so the cost on the error-hot path is
    /// negligible.
    ///
    /// Full diagnostic detail is still preserved on the server side
    /// via `tracing` events that have been written through the same
    /// redactor (see `crate::diagnostics::sanitize_error_message`).
    pub fn from_mukei_error(error: &MukeiError, source: impl Into<String>) -> Self {
        let severity = severity_for(error);
        let suggested_action = suggested_action_for(error);
        let recoverable = recoverable_for(error);
        let technical_message =
            crate::diagnostics::sanitize_error_message(error.to_string());
        Self {
            code: error.error_code().to_string(),
            class: error.classification().to_string(),
            severity,
            recoverable,
            user_message: user_message_for(error),
            technical_message,
            suggested_action,
            source: source.into(),
        }
    }

    /// Variant that lets callers pre-redact the technical message (e.g.
    /// because they want to combine the error with a structured
    /// diagnostic blob). Defaults `from_mukei_error` already redacts,
    /// but this entry point is convenient when an embedder has its own
    /// adapter that wants to surface only a substr of the original
    /// error string.
    pub fn from_mukei_error_redacted(
        error: &MukeiError,
        source: impl Into<String>,
        technical_message: impl Into<String>,
    ) -> Self {
        let severity = severity_for(error);
        let suggested_action = suggested_action_for(error);
        let recoverable = recoverable_for(error);
        let technical_message =
            crate::diagnostics::sanitize_error_message(technical_message.into());
        Self {
            code: error.error_code().to_string(),
            class: error.classification().to_string(),
            severity,
            recoverable,
            user_message: user_message_for(error),
            technical_message,
            suggested_action,
            source: source.into(),
        }
    }
}

/// Snapshot of actions the UI may currently offer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySnapshot {
    /// Whether initialization can be requested.
    pub can_initialize: bool,
    /// Whether a chat message can be submitted.
    pub can_send_message: bool,
    /// Whether active generation can be stopped.
    pub can_stop_generation: bool,
    /// Whether a model download can be started.
    pub can_download_model: bool,
    /// Whether an active model download can be stopped.
    pub can_stop_download: bool,
    /// Whether the active model can be switched.
    pub can_switch_model: bool,
    /// Whether a local model can be deleted.
    pub can_delete_model: bool,
    /// Whether the active conversation can be cleared.
    pub can_clear_conversation: bool,
    /// Whether settings can be opened.
    pub can_open_settings: bool,
    /// Whether valid config is still needed.
    pub needs_config: bool,
    /// Whether Android storage or SAF permission is needed.
    pub needs_storage_permission: bool,
    /// Whether an active model is ready for inference.
    pub active_model_ready: bool,
    /// Whether any exclusive backend operation is active.
    pub is_busy: bool,
    /// Whether a model download is active.
    pub is_downloading: bool,
    /// Whether inference is active.
    pub is_inferencing: bool,
}

impl CapabilitySnapshot {
    fn network_enabled() -> bool {
        cfg!(feature = "network")
    }

    /// Conservative uninitialized capability set.
    pub fn uninitialized() -> Self {
        Self {
            can_initialize: true,
            can_send_message: false,
            can_stop_generation: false,
            can_download_model: false,
            can_stop_download: false,
            can_switch_model: false,
            can_delete_model: false,
            can_clear_conversation: false,
            can_open_settings: true,
            needs_config: false,
            needs_storage_permission: false,
            active_model_ready: false,
            is_busy: false,
            is_downloading: false,
            is_inferencing: false,
        }
    }

    /// Ready capability set for the current bridge implementation.
    ///
    /// This means the bridge runtime is initialized and can accept UI
    /// commands. It does not prove a GGUF has been verified or loaded.
    pub fn ready() -> Self {
        Self {
            can_initialize: false,
            can_send_message: true,
            can_stop_generation: false,
            can_download_model: Self::network_enabled(),
            can_stop_download: false,
            can_switch_model: true,
            can_delete_model: true,
            can_clear_conversation: true,
            can_open_settings: true,
            needs_config: false,
            needs_storage_permission: false,
            active_model_ready: false,
            is_busy: false,
            is_downloading: false,
            is_inferencing: false,
        }
    }

    /// Inferencing capability set.
    ///
    /// Use only after a chat turn has passed the bridge busy guard and
    /// the bridge has accepted the request for execution.
    pub fn inferencing() -> Self {
        Self {
            can_initialize: false,
            can_send_message: false,
            can_stop_generation: true,
            can_download_model: Self::network_enabled(),
            can_stop_download: false,
            can_switch_model: false,
            can_delete_model: false,
            can_clear_conversation: false,
            can_open_settings: true,
            needs_config: false,
            needs_storage_permission: false,
            active_model_ready: false,
            is_busy: true,
            is_downloading: false,
            is_inferencing: true,
        }
    }

    /// Downloading capability set.
    ///
    /// `active_model_ready` must be supplied by the caller. The current
    /// bridge generally passes `false` because model health is not yet
    /// tracked independently from download state.
    pub fn downloading(active_model_ready: bool) -> Self {
        Self {
            can_initialize: false,
            can_send_message: active_model_ready,
            can_stop_generation: false,
            can_download_model: Self::network_enabled(),
            can_stop_download: Self::network_enabled(),
            can_switch_model: false,
            can_delete_model: false,
            can_clear_conversation: active_model_ready,
            can_open_settings: true,
            needs_config: false,
            needs_storage_permission: false,
            active_model_ready,
            is_busy: true,
            is_downloading: true,
            is_inferencing: false,
        }
    }

    /// Capability set for config-required startup.
    pub fn needs_config() -> Self {
        let mut snapshot = Self::uninitialized();
        snapshot.needs_config = true;
        snapshot.can_initialize = false;
        snapshot
    }
}

/// Stable event envelope emitted by the bridge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BridgeEvent {
    /// Schema version for forward-compatible QML parsing.
    pub schema_version: u32,
    /// UTC event timestamp.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Optional conversation id when a chat turn is involved.
    ///
    /// In the current bridge integration this id is bridge-local and
    /// process-local. It is not yet the persisted conversation id from
    /// storage, so QML must treat it as an event-correlation id only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<ConversationId>,
    /// Optional turn id when a chat turn is involved.
    ///
    /// In the current bridge integration this id is generated by the
    /// bridge for the live turn and is not yet persisted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// Optional message id when a specific message is involved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<MessageId>,
    /// Optional monotonically increasing sequence number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    /// Event-specific payload.
    #[serde(flatten)]
    pub kind: BridgeEventKind,
}

impl BridgeEvent {
    /// Current event schema version.
    pub const SCHEMA_VERSION: u32 = 1;

    /// Construct an event with current timestamp and no optional ids.
    pub fn new(kind: BridgeEventKind) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            timestamp: chrono::Utc::now(),
            conversation_id: None,
            turn_id: None,
            message_id: None,
            sequence: None,
            kind,
        }
    }

    /// Attach chat ids to an event.
    pub fn with_chat_context(
        mut self,
        conversation_id: ConversationId,
        turn_id: impl Into<String>,
    ) -> Self {
        self.conversation_id = Some(conversation_id);
        self.turn_id = Some(turn_id.into());
        self
    }

    /// Attach a message id to an event.
    pub fn with_message_id(mut self, message_id: MessageId) -> Self {
        self.message_id = Some(message_id);
        self
    }

    /// Attach a sequence number to an event.
    pub fn with_sequence(mut self, sequence: u64) -> Self {
        self.sequence = Some(sequence);
        self
    }
}

/// Event category and payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "category", rename_all = "snake_case")]
pub enum BridgeEventKind {
    /// Application lifecycle changed.
    AppLifecycle {
        /// New lifecycle state.
        state: AppLifecycleState,
        /// Current capabilities.
        capabilities: CapabilitySnapshot,
        /// Android storage/SAF state when available.
        #[serde(skip_serializing_if = "Option::is_none")]
        android_storage: Option<AndroidStorageState>,
    },
    /// Capability snapshot changed without another state transition.
    CapabilitySnapshot {
        /// Current capabilities.
        capabilities: CapabilitySnapshot,
    },
    /// Chat turn state changed.
    ChatState {
        /// New chat state.
        state: ChatTurnState,
        /// Current capabilities.
        capabilities: CapabilitySnapshot,
    },
    /// One assistant chunk arrived.
    ChatChunk {
        /// Chunk text.
        chunk: String,
    },
    /// Chat turn completed.
    ChatCompleted,
    /// Chat turn was cancelled.
    ChatCancelled,
    /// Chat turn failed.
    ChatFailed {
        /// UI-friendly error.
        error: UiError,
    },
    /// Download state changed.
    DownloadState {
        /// New download state.
        state: DownloadState,
        /// Optional model id when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        model_id: Option<String>,
        /// Optional destination path when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        destination: Option<String>,
        /// Current capabilities.
        capabilities: CapabilitySnapshot,
    },
    /// Download progress changed.
    DownloadProgress {
        /// Download lifecycle state.
        state: DownloadState,
        /// Fraction in `[0.0, 1.0]`.
        progress: f64,
        /// Bytes downloaded so far.
        bytes_downloaded: u64,
        /// Total bytes if known.
        #[serde(skip_serializing_if = "Option::is_none")]
        total_bytes: Option<u64>,
        /// Optional model id when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        model_id: Option<String>,
        /// Optional destination path when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        destination: Option<String>,
    },
    /// Download completed.
    DownloadCompleted {
        /// Verified final path.
        final_path: String,
    },
    /// Download failed.
    DownloadFailed {
        /// UI-friendly error.
        error: UiError,
    },
    /// Generic error event.
    Error {
        /// UI-friendly error.
        error: UiError,
    },
}

fn severity_for(error: &MukeiError) -> ErrorSeverity {
    match error {
        MukeiError::FFIPanic
        | MukeiError::SecretLeaked(_)
        | MukeiError::DatabaseCorruption
        | MukeiError::MigrationOrderConflict { .. }
        | MukeiError::AuditLogTampered { .. }
        | MukeiError::CrashLoopDetected { .. }
        | MukeiError::Invariant(_) => ErrorSeverity::Fatal,
        MukeiError::Cancelled | MukeiError::BridgeBusy | MukeiError::DownloadBusy { .. } => {
            ErrorSeverity::Info
        }
        MukeiError::ThermalThrottle
        | MukeiError::ToolTimeout(_)
        | MukeiError::ToolArgsRejected { .. }
        | MukeiError::ToolAbuseBlocked { .. }
        | MukeiError::SafRequired
        | MukeiError::SafRevoked
        | MukeiError::PermissionDenied => ErrorSeverity::Warning,
        _ => ErrorSeverity::Error,
    }
}

fn recoverable_for(error: &MukeiError) -> bool {
    !matches!(
        error,
        MukeiError::FFIPanic
            | MukeiError::SecretLeaked(_)
            | MukeiError::DatabaseCorruption
            | MukeiError::MigrationOrderConflict { .. }
            | MukeiError::AuditLogTampered { .. }
            | MukeiError::CrashLoopDetected { .. }
            | MukeiError::Invariant(_)
    )
}

fn suggested_action_for(error: &MukeiError) -> SuggestedUiAction {
    match error {
        MukeiError::ConfigMissingField(_)
        | MukeiError::ConfigInvalid { .. }
        | MukeiError::ConfigUnknownField(_)
        | MukeiError::SafeStorageUnavailable(_)
        | MukeiError::WrappedKeyMalformed(_)
        | MukeiError::UnwrapFailed => SuggestedUiAction::OpenSettings,
        MukeiError::SafRequired | MukeiError::SafRevoked | MukeiError::PermissionDenied => {
            SuggestedUiAction::RequestStoragePermission
        }
        MukeiError::ModelLoadFailed(_)
        | MukeiError::ModelCorrupted
        | MukeiError::DownloadHashMismatch
        | MukeiError::DownloadSizeMissing
        | MukeiError::DownloadTooLarge { .. }
        | MukeiError::MemoryPreflightRejected(_) => SuggestedUiAction::OpenModelManager,
        MukeiError::NetworkError(_)
        | MukeiError::NetworkTimeout { .. }
        | MukeiError::NetworkUnavailable { .. }
        | MukeiError::NetworkTls { .. }
        | MukeiError::NetworkInvalidResponse { .. }
        | MukeiError::NetworkRateLimited { .. }
        | MukeiError::NetworkServerError { .. }
        | MukeiError::HttpClientFailed(_)
        | MukeiError::Io(_)
        | MukeiError::ToolTimeout(_)
        | MukeiError::WebSearchFailed(_) => SuggestedUiAction::Retry,
        MukeiError::RemoteFeatureDisabled { .. } => SuggestedUiAction::OpenSettings,
        MukeiError::BridgeBusy => SuggestedUiAction::StopGeneration,
        MukeiError::DownloadBusy { .. } | MukeiError::Cancelled => SuggestedUiAction::ClearDownload,
        MukeiError::FFIPanic
        | MukeiError::DatabaseCorruption
        | MukeiError::MigrationOrderConflict { .. }
        | MukeiError::AuditLogTampered { .. }
        | MukeiError::CrashLoopDetected { .. } => SuggestedUiAction::RestartApp,
        MukeiError::SecretLeaked(_) | MukeiError::PromptLeakage | MukeiError::Invariant(_) => {
            SuggestedUiAction::ReportIssue
        }
        _ => SuggestedUiAction::None,
    }
}

fn user_message_for(error: &MukeiError) -> String {
    match error {
        MukeiError::ConfigMissingField(_)
        | MukeiError::ConfigInvalid { .. }
        | MukeiError::ConfigUnknownField(_) => "Configuration needs attention.".to_string(),
        MukeiError::DatabaseInitFailed(_)
        | MukeiError::DatabaseCorruption
        | MukeiError::MigrationFailed(_, _)
        | MukeiError::MigrationOrderConflict { .. }
        | MukeiError::AuditLogTampered { .. }
        | MukeiError::DatabaseEncryptionUnavailable
        | MukeiError::DatabaseEncryptionMigrationRequired
        | MukeiError::DatabaseEncryptionInvalidKey
        | MukeiError::DatabaseEncryptionCorrupted => {
            "Local storage could not be opened safely.".to_string()
        }
        MukeiError::SafRequired => "Storage permission is required for this file.".to_string(),
        MukeiError::SafRevoked => "Storage access was revoked.".to_string(),
        MukeiError::PermissionDenied => "Permission was denied.".to_string(),
        MukeiError::ModelLoadFailed(_) | MukeiError::ModelCorrupted => {
            "The selected model is not ready.".to_string()
        }
        MukeiError::DownloadHashMismatch => {
            "The downloaded model did not pass verification.".to_string()
        }
        MukeiError::DownloadSizeMissing | MukeiError::DownloadTooLarge { .. } => {
            "The model download could not be accepted safely.".to_string()
        }
        MukeiError::NetworkError(_)
        | MukeiError::NetworkTimeout { .. }
        | MukeiError::NetworkUnavailable { .. }
        | MukeiError::NetworkTls { .. }
        | MukeiError::NetworkInvalidResponse { .. }
        | MukeiError::NetworkRateLimited { .. }
        | MukeiError::NetworkServerError { .. }
        | MukeiError::HttpClientFailed(_) => "Network request failed.".to_string(),
        MukeiError::RemoteFeatureDisabled { .. } => {
            "Remote features are disabled by privacy settings.".to_string()
        }
        MukeiError::BridgeBusy => "A response is already running.".to_string(),
        MukeiError::DownloadBusy { .. } => "That model is already downloading.".to_string(),
        MukeiError::Cancelled => "Operation cancelled.".to_string(),
        MukeiError::OOM | MukeiError::MemoryPreflightRejected(_) => {
            "The device does not have enough available memory.".to_string()
        }
        MukeiError::ThermalThrottle => "The device is too hot to continue safely.".to_string(),
        _ => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_lifecycle_state_serializes_as_snake_case() {
        let json = serde_json::to_string(&AppLifecycleState::OpeningDatabase).unwrap();
        assert_eq!(json, "\"opening_database\"");
    }

    #[test]
    fn chat_turn_state_serializes_as_snake_case() {
        let json = serde_json::to_string(&ChatTurnState::ToolCalling).unwrap();
        assert_eq!(json, "\"tool_calling\"");
    }

    #[test]
    fn download_state_serializes_as_snake_case() {
        let json = serde_json::to_string(&DownloadState::Downloading).unwrap();
        assert_eq!(json, "\"downloading\"");
    }

    #[test]
    fn bridge_event_serializes_stable_snake_case_envelope() {
        let event = BridgeEvent::new(BridgeEventKind::AppLifecycle {
            state: AppLifecycleState::Ready,
            capabilities: CapabilitySnapshot::ready(),
            android_storage: Some(AndroidStorageState::Ready { saf_grant_count: 2 }),
        });
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["category"], "app_lifecycle");
        assert_eq!(value["state"], "ready");
        assert_eq!(value["android_storage"]["state"], "ready");
        assert_eq!(value["android_storage"]["saf_grant_count"], 2);
        assert!(value.get("timestamp").is_some());
    }

    #[test]
    fn download_progress_event_has_typed_fields() {
        let event = BridgeEvent::new(BridgeEventKind::DownloadProgress {
            state: DownloadState::Downloading,
            progress: 0.25,
            bytes_downloaded: 512,
            total_bytes: Some(2048),
            model_id: Some("gemma-4-e2b-it".into()),
            destination: Some("/models/gemma.gguf".into()),
        });
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["category"], "download_progress");
        assert_eq!(value["state"], "downloading");
        assert_eq!(value["bytes_downloaded"], 512);
        assert_eq!(value["total_bytes"], 2048);
    }

    #[test]
    fn ui_error_preserves_code_and_classification() {
        let err = MukeiError::ConfigInvalid {
            field: "n_ctx".into(),
            reason: "too small".into(),
        };
        let ui = UiError::from_mukei_error(&err, "initialize");
        assert_eq!(ui.code, "ERR_CONFIG_INVALID");
        assert_eq!(ui.class, "config");
        assert_eq!(ui.severity, ErrorSeverity::Error);
        assert!(ui.recoverable);
        assert!(ui.technical_message.contains("n_ctx"));
        assert_eq!(ui.suggested_action, SuggestedUiAction::OpenSettings);
    }

    #[test]
    fn ui_error_technical_message_redacts_secrets_and_paths() {
        // Simulate an upstream crate whose Display impl embeds a raw
        // path AND a leaked bearer token. Without sanitization the
        // exact strings would pass verbatim into the QML diagnostics
        // panel — exactly the case the v0.8 review flagged.
        let err = MukeiError::Internal(String::from(
            "upstream: Authorization: Bearer abc123 path=/sdcard/Documents/private.txt",
        ));
        let ui = UiError::from_mukei_error(&err, "boot");
        assert!(
            !ui.technical_message.contains("Bearer abc123"),
            "technical_message must redact leaked bearer token: {}",
            ui.technical_message
        );
        assert!(
            !ui.technical_message.contains("/sdcard/Documents/private.txt"),
            "technical_message must redact leaked absolute path: {}",
            ui.technical_message
        );
        assert!(
            ui.technical_message.contains("[redacted-"),
            "technical_message should clearly mark redactions: {}",
            ui.technical_message
        );
    }

    #[test]
    fn ui_error_pre_redacted_entry_point_still_sanitises() {
        let err = MukeiError::NetworkError(String::new());
        let ui = UiError::from_mukei_error_redacted(
            &err,
            "boot",
            "context=/tmp/foo api_key=leaked",
        );
        assert!(!ui.technical_message.contains("leaked"));
        assert!(
            ui.technical_message.contains("[redacted-"),
            "sanitizer output should mark redactions: {}",
            ui.technical_message
        );
    }

    #[test]
    fn unsafe_download_size_errors_route_to_model_manager() {
        for err in [
            MukeiError::DownloadSizeMissing,
            MukeiError::DownloadTooLarge {
                max_bytes: 16,
                actual_bytes: 17,
            },
        ] {
            let ui = UiError::from_mukei_error(&err, "download_model");
            let expected_class = match err {
                MukeiError::DownloadTooLarge { .. } => "resource",
                _ => "network",
            };
            assert_eq!(ui.class, expected_class);
            assert_eq!(ui.severity, ErrorSeverity::Error);
            assert!(ui.recoverable);
            assert_eq!(ui.suggested_action, SuggestedUiAction::OpenModelManager);
            assert_eq!(
                ui.user_message,
                "The model download could not be accepted safely."
            );
        }
    }

    #[test]
    fn typed_network_errors_are_retryable_for_ui() {
        for err in [
            MukeiError::NetworkTimeout {
                operation: "download".into(),
            },
            MukeiError::NetworkRateLimited {
                operation: "download".into(),
            },
            MukeiError::NetworkServerError {
                status: 503,
                operation: "download".into(),
            },
        ] {
            let ui = UiError::from_mukei_error(&err, "download_model");
            assert_eq!(ui.class, "network");
            assert!(ui.recoverable);
            assert_eq!(ui.suggested_action, SuggestedUiAction::Retry);
            assert_eq!(ui.user_message, "Network request failed.");
        }
    }

    #[test]
    fn remote_disabled_error_routes_to_settings() {
        let err = MukeiError::RemoteFeatureDisabled {
            feature: "web_search",
            policy: "local_only".into(),
        };
        let ui = UiError::from_mukei_error(&err, "web_search");
        assert_eq!(ui.code, "ERR_REMOTE_DISABLED");
        assert_eq!(ui.class, "permission");
        assert!(ui.recoverable);
        assert_eq!(ui.suggested_action, SuggestedUiAction::OpenSettings);
        assert_eq!(
            ui.user_message,
            "Remote features are disabled by privacy settings."
        );
    }

    #[test]
    fn capability_snapshots_cover_core_runtime_modes() {
        let uninitialized = CapabilitySnapshot::uninitialized();
        assert!(uninitialized.can_initialize);
        assert!(!uninitialized.can_send_message);

        let ready = CapabilitySnapshot::ready();
        assert!(ready.can_send_message);
        assert_eq!(
            ready.can_download_model,
            CapabilitySnapshot::network_enabled()
        );
        assert!(!ready.active_model_ready);
        assert!(!ready.is_busy);

        let inferencing = CapabilitySnapshot::inferencing();
        assert!(inferencing.can_stop_generation);
        assert!(inferencing.is_busy);
        assert!(inferencing.is_inferencing);
        assert!(!inferencing.active_model_ready);

        let downloading = CapabilitySnapshot::downloading(false);
        assert_eq!(
            downloading.can_stop_download,
            CapabilitySnapshot::network_enabled()
        );
        assert!(downloading.is_downloading);
        assert!(!downloading.can_send_message);
    }

    #[test]
    fn bridge_chat_ids_are_documented_as_process_local() {
        let event = BridgeEvent::new(BridgeEventKind::ChatState {
            state: ChatTurnState::Submitting,
            capabilities: CapabilitySnapshot::inferencing(),
        })
        .with_chat_context(ConversationId::new(), MessageId::new().0.to_string());
        let json = serde_json::to_value(event).unwrap();
        assert!(json.get("conversation_id").is_some());
        assert!(json.get("turn_id").is_some());
    }

    #[test]
    fn android_saf_state_is_representable() {
        let state = AndroidStorageState::SafPermissionRequired;
        let value = serde_json::to_value(state).unwrap();
        assert_eq!(value["state"], "saf_permission_required");
    }

    #[test]
    fn download_terminal_states_preserve_model_identity() {
        let failed = BridgeEvent::new(BridgeEventKind::DownloadState {
            state: DownloadState::Failed,
            model_id: Some("gemma-4-e2b-it".into()),
            destination: Some("/models/gemma.gguf".into()),
            capabilities: CapabilitySnapshot::ready(),
        });
        let cancelled = BridgeEvent::new(BridgeEventKind::DownloadState {
            state: DownloadState::Cancelled,
            model_id: Some("gemma-4-e2b-it".into()),
            destination: Some("/models/gemma.gguf".into()),
            capabilities: CapabilitySnapshot::ready(),
        });

        let failed_json = serde_json::to_value(failed).unwrap();
        let cancelled_json = serde_json::to_value(cancelled).unwrap();
        assert_eq!(failed_json["state"], "failed");
        assert_eq!(cancelled_json["state"], "cancelled");
        assert_eq!(failed_json["model_id"], "gemma-4-e2b-it");
        assert_eq!(cancelled_json["model_id"], "gemma-4-e2b-it");
        assert_eq!(failed_json["destination"], "/models/gemma.gguf");
        assert_eq!(cancelled_json["destination"], "/models/gemma.gguf");
    }

    #[test]
    fn error_event_has_typed_payload_for_qml_dispatch() {
        let error = UiError::from_mukei_error(
            &MukeiError::NetworkError("timeout".into()),
            "download_model",
        );
        let event = BridgeEvent::new(BridgeEventKind::Error { error });
        let value = serde_json::to_value(event).unwrap();

        assert_eq!(value["category"], "error");
        assert_eq!(value["error"]["code"], "ERR_NETWORK");
        assert_eq!(value["error"]["source"], "download_model");
    }

    #[test]
    fn audit_tamper_error_is_fatal_and_stable() {
        let ui =
            UiError::from_mukei_error(&MukeiError::AuditLogTampered { row_id: 7 }, "initialize");
        assert_eq!(ui.code, "ERR_AUDIT_TAMPERED");
        assert_eq!(ui.class, "storage");
        assert_eq!(ui.severity, ErrorSeverity::Fatal);
        assert!(!ui.recoverable);
        assert_eq!(ui.suggested_action, SuggestedUiAction::RestartApp);
        assert_eq!(ui.user_message, "Local storage could not be opened safely.");
    }
}
