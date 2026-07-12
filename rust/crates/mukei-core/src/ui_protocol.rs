//! Versioned command/acknowledgement/event protocol shared by the local bridge and QML UI.
//!
//! The protocol is intentionally transport-neutral. It runs over the existing in-process
//! CXX-Qt bridge today and keeps opaque string identifiers so the same envelopes can later be
//! carried over a remote transport without changing UI lifecycle semantics.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Current protocol major version. Unknown majors are incompatible and must fail closed.
pub const PROTOCOL_MAJOR: u16 = 2;
/// Current protocol minor version. Minor additions are backward-compatible optional fields.
pub const PROTOCOL_MINOR: u16 = 0;
/// Oldest peer major supported by this implementation.
pub const MIN_SUPPORTED_PEER_MAJOR: u16 = 2;
/// Maximum serialized command envelope size accepted at the bridge boundary.
pub const MAX_COMMAND_ENVELOPE_BYTES: usize = 64 * 1024;
/// Maximum opaque protocol identifier length.
pub const MAX_PROTOCOL_ID_LEN: usize = 128;
/// Maximum command type length.
pub const MAX_COMMAND_TYPE_LEN: usize = 96;
/// Maximum idempotency key length.
pub const MAX_IDEMPOTENCY_KEY_LEN: usize = 192;

/// Stable machine capability: protocol-v2 command envelope support.
pub const CAP_COMMAND_ENVELOPE_V2: &str = "command_envelope_v2";
/// Stable machine capability: immediate accepted/rejected command acknowledgement.
pub const CAP_COMMAND_ACKNOWLEDGEMENT: &str = "command_acknowledgement";
/// Stable machine capability: globally unique event identity.
pub const CAP_EVENT_IDENTITY: &str = "event_identity";
/// Stable machine capability: sequencing is monotonic within each stream.
pub const CAP_PER_STREAM_SEQUENCING: &str = "per_stream_sequencing";
/// Stable machine capability: bounded replay protection for idempotent commands.
pub const CAP_IDEMPOTENT_COMMAND_REPLAY: &str = "idempotent_command_replay";
/// Stable machine capability: command-correlated operation lifecycle projection.
pub const CAP_OPERATION_LIFECYCLE_EVENTS: &str = "operation_lifecycle_events";
/// Stable machine capability: chat cancellation targets an explicit scoped operation.
pub const CAP_SCOPED_CHAT_OPERATIONS: &str = "scoped_chat_operations";
/// Stable machine capability: isolated legacy-v1 event ingestion remains
/// available during transition.
pub const CAP_LEGACY_EVENT_V1_COMPATIBILITY: &str = "legacy_event_v1_compatibility";

/// Protocol version carried by every v2 command, acknowledgement, and event envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    /// Incompatible protocol generation.
    pub major: u16,
    /// Backward-compatible optional-field generation.
    pub minor: u16,
}

impl ProtocolVersion {
    /// Current protocol version.
    pub const CURRENT: Self = Self {
        major: PROTOCOL_MAJOR,
        minor: PROTOCOL_MINOR,
    };

    /// Whether this implementation can safely consume a peer version.
    pub fn is_compatible(self) -> bool {
        self.major == PROTOCOL_MAJOR && self.major >= MIN_SUPPORTED_PEER_MAJOR
    }
}

/// Protocol-level capability snapshot used during UI/bridge negotiation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolCapabilitySnapshot {
    /// Current local protocol version.
    pub current_version: ProtocolVersion,
    /// Minimum peer major accepted by this implementation.
    pub minimum_supported_peer_major: u16,
    /// Stable machine capability names that are actually implemented.
    pub capabilities: Vec<String>,
}

impl ProtocolCapabilitySnapshot {
    /// Current local capability set.
    pub fn current() -> Self {
        Self {
            current_version: ProtocolVersion::CURRENT,
            minimum_supported_peer_major: MIN_SUPPORTED_PEER_MAJOR,
            capabilities: vec![
                CAP_COMMAND_ENVELOPE_V2.into(),
                CAP_COMMAND_ACKNOWLEDGEMENT.into(),
                CAP_EVENT_IDENTITY.into(),
                CAP_PER_STREAM_SEQUENCING.into(),
                CAP_IDEMPOTENT_COMMAND_REPLAY.into(),
                CAP_OPERATION_LIFECYCLE_EVENTS.into(),
                CAP_SCOPED_CHAT_OPERATIONS.into(),
                CAP_LEGACY_EVENT_V1_COMPATIBILITY.into(),
            ],
        }
    }
}

/// Structured logical scope. Domain identifiers remain opaque strings at the serialized boundary.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandScope {
    /// Optional conversation identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// Optional branch identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_id: Option<String>,
    /// Optional turn identifier for scoped chat operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// Optional model identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// Optional document identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
}

/// Serialized protocol-v2 command envelope accepted by the bridge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandEnvelopeV2 {
    /// Protocol version.
    pub protocol_version: ProtocolVersion,
    /// Stable unique identity of this command envelope.
    pub command_id: String,
    /// Identity of this submission request.
    pub request_id: String,
    /// Stable machine command type.
    pub command_type: String,
    /// Client submission time.
    pub submitted_at: DateTime<Utc>,
    /// Existing or client-proposed operation identity when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// Correlation identity shared by acknowledgement and resulting events.
    pub correlation_id: String,
    /// Replay-protection key for commands that support safe resubmission.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Optional logical scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<CommandScope>,
    /// Structured payload validated against `command_type` before dispatch.
    pub payload: Value,
}

/// Canonical registry of backend command types crossing the QML/Rust boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandType {
    /// Initialize the local runtime.
    AppInitialize,
    /// Submit a chat message.
    ChatSendMessage,
    /// Request cancellation of the active chat turn.
    ChatStopGeneration,
    /// Clear the active conversation session.
    ChatClearConversation,
    /// Start a model download.
    ModelDownload,
    /// Cancel active model downloads.
    DownloadCancel,
    /// Select an installed model.
    ModelSelect,
    /// Delete an installed model.
    ModelDelete,
    /// Grant private document access.
    DocumentGrant,
    /// Revoke a private document.
    DocumentRevoke,
    /// Retry document ingestion.
    DocumentRetryIngestion,
    /// Persist a UI setting.
    SettingsUpdate,
    /// Resume an interrupted response.
    RecoveryResume,
    /// Regenerate an interrupted response.
    RecoveryRegenerate,
}

impl CommandType {
    /// Parse one stable machine command type.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "app.initialize" => Some(Self::AppInitialize),
            "chat.send_message" => Some(Self::ChatSendMessage),
            "chat.stop_generation" => Some(Self::ChatStopGeneration),
            "chat.clear_conversation" => Some(Self::ChatClearConversation),
            "model.download" => Some(Self::ModelDownload),
            "download.cancel" => Some(Self::DownloadCancel),
            "model.select" => Some(Self::ModelSelect),
            "model.delete" => Some(Self::ModelDelete),
            "document.grant" => Some(Self::DocumentGrant),
            "document.revoke" => Some(Self::DocumentRevoke),
            "document.retry_ingestion" => Some(Self::DocumentRetryIngestion),
            "settings.update" => Some(Self::SettingsUpdate),
            "recovery.resume" => Some(Self::RecoveryResume),
            "recovery.regenerate" => Some(Self::RecoveryRegenerate),
            _ => None,
        }
    }

    /// Stable serialized machine string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AppInitialize => "app.initialize",
            Self::ChatSendMessage => "chat.send_message",
            Self::ChatStopGeneration => "chat.stop_generation",
            Self::ChatClearConversation => "chat.clear_conversation",
            Self::ModelDownload => "model.download",
            Self::DownloadCancel => "download.cancel",
            Self::ModelSelect => "model.select",
            Self::ModelDelete => "model.delete",
            Self::DocumentGrant => "document.grant",
            Self::DocumentRevoke => "document.revoke",
            Self::DocumentRetryIngestion => "document.retry_ingestion",
            Self::SettingsUpdate => "settings.update",
            Self::RecoveryResume => "recovery.resume",
            Self::RecoveryRegenerate => "recovery.regenerate",
        }
    }

    /// Whether replay protection is mandatory for this command type.
    pub const fn requires_idempotency_key(self) -> bool {
        matches!(
            self,
            Self::ChatSendMessage
                | Self::ModelDownload
                | Self::ModelDelete
                | Self::DocumentGrant
                | Self::DocumentRevoke
                | Self::DocumentRetryIngestion
                | Self::RecoveryResume
                | Self::RecoveryRegenerate
        )
    }

    /// Whether the command creates or represents an operation lifecycle.
    pub const fn creates_operation(self) -> bool {
        true
    }
}

/// Payload for runtime initialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializePayload {
    /// App-private configuration path.
    pub config_path: String,
}

/// Payload for chat submission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendMessagePayload {
    /// User text to submit.
    pub text: String,
}

/// Payload containing one model identifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPayload {
    /// Canonical model identifier.
    pub model_id: String,
}

/// Payload for a model download.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDownloadPayload {
    /// Canonical model identifier or approved HTTPS source understood by the existing bridge.
    pub model_id: String,
    /// Optional pinned SHA-256 for bespoke QA sources.
    #[serde(default)]
    pub sha256: String,
}

/// Payload for private document grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentGrantPayload {
    /// Opaque SAF/app-private target.
    pub target: String,
    /// User-visible document label.
    pub label: String,
    /// MIME type.
    pub mime_type: String,
}

/// Payload containing one document identifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentPayload {
    /// Opaque document identifier.
    pub document_id: String,
}

/// Payload for one setting mutation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingUpdatePayload {
    /// Stable setting key.
    pub key: String,
    /// JSON scalar setting value.
    pub value: Value,
}

/// Validated typed command payload.
#[derive(Clone, Debug, PartialEq)]
pub enum ValidatedCommandPayload {
    /// No payload fields.
    Empty,
    /// Runtime initialization payload.
    Initialize(InitializePayload),
    /// Chat message payload.
    SendMessage(SendMessagePayload),
    /// Model download payload.
    ModelDownload(ModelDownloadPayload),
    /// Model identifier payload.
    Model(ModelPayload),
    /// Document grant payload.
    DocumentGrant(DocumentGrantPayload),
    /// Document identifier payload.
    Document(DocumentPayload),
    /// Setting mutation payload.
    SettingUpdate(SettingUpdatePayload),
}

/// Structurally validated command ready for bridge-side policy preflight and dispatch.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedCommand {
    /// Original envelope.
    pub envelope: CommandEnvelopeV2,
    /// Registry command type.
    pub command_type: CommandType,
    /// Type-checked payload.
    pub payload: ValidatedCommandPayload,
}

/// Stable machine acknowledgement status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcknowledgementStatus {
    /// Validated and accepted for processing; does not imply completion.
    Accepted,
    /// Rejected before execution.
    Rejected,
}

/// Stable machine-readable command rejection reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    /// Protocol major is unsupported.
    UnsupportedProtocol,
    /// Command type is unknown.
    UnknownCommand,
    /// Envelope or typed payload is malformed.
    InvalidPayload,
    /// Required capability is not currently available.
    CapabilityUnavailable,
    /// Conflicting/busy operation prevents acceptance.
    BusyConflict,
    /// Logical scope is invalid or no longer current.
    StaleScope,
    /// Local backend is unavailable.
    BackendUnavailable,
    /// Idempotency key was reused for different command content.
    DuplicateReplayConflict,
    /// Local policy denied the command.
    PolicyDenied,
}

/// Immediate command acknowledgement envelope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandAcknowledgementV2 {
    /// Protocol version.
    pub protocol_version: ProtocolVersion,
    /// Command identity copied from the request.
    pub command_id: String,
    /// Request identity copied from the request.
    pub request_id: String,
    /// Correlation identity copied from the request.
    pub correlation_id: String,
    /// Allocated or targeted operation identity when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// Accepted or rejected.
    pub status: AcknowledgementStatus,
    /// Machine rejection reason when rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<RejectionReason>,
    /// Acknowledgement timestamp.
    pub timestamp: DateTime<Utc>,
}

impl CommandAcknowledgementV2 {
    /// Build an accepted acknowledgement.
    pub fn accepted(envelope: &CommandEnvelopeV2, operation_id: Option<String>) -> Self {
        Self {
            protocol_version: ProtocolVersion::CURRENT,
            command_id: envelope.command_id.clone(),
            request_id: envelope.request_id.clone(),
            correlation_id: envelope.correlation_id.clone(),
            operation_id,
            status: AcknowledgementStatus::Accepted,
            rejection_reason: None,
            timestamp: Utc::now(),
        }
    }

    /// Build a rejected acknowledgement. Missing malformed IDs are echoed as empty strings.
    pub fn rejected(envelope: Option<&CommandEnvelopeV2>, reason: RejectionReason) -> Self {
        Self {
            protocol_version: ProtocolVersion::CURRENT,
            command_id: envelope.map(|v| v.command_id.clone()).unwrap_or_default(),
            request_id: envelope.map(|v| v.request_id.clone()).unwrap_or_default(),
            correlation_id: envelope
                .map(|v| v.correlation_id.clone())
                .unwrap_or_default(),
            operation_id: None,
            status: AcknowledgementStatus::Rejected,
            rejection_reason: Some(reason),
            timestamp: Utc::now(),
        }
    }

    /// Validate that this acknowledgement belongs to `command` and is structurally complete.
    pub fn validate_for(&self, command: &CommandEnvelopeV2) -> bool {
        if self.protocol_version.major != ProtocolVersion::CURRENT.major
            || self.command_id != command.command_id
            || self.request_id != command.request_id
            || self.correlation_id != command.correlation_id
        {
            return false;
        }
        match self.status {
            AcknowledgementStatus::Accepted => self
                .operation_id
                .as_deref()
                .is_some_and(|value| valid_protocol_id(value, MAX_PROTOCOL_ID_LEN)),
            AcknowledgementStatus::Rejected => self.rejection_reason.is_some(),
        }
    }
}

/// Protocol-v2 reliable event envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelopeV2 {
    /// Protocol version.
    pub protocol_version: ProtocolVersion,
    /// Globally unique event identity.
    pub event_id: String,
    /// Ordered logical stream identity.
    pub stream_id: String,
    /// Monotonic sequence within `stream_id`.
    pub sequence: u64,
    /// Stable event category/type.
    pub event_type: String,
    /// Event emission timestamp.
    pub emitted_at: DateTime<Utc>,
    /// Correlation identity when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Operation identity when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// Direct request identity when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Direct command identity when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    /// Originating command type when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_type: Option<String>,
    /// Structured event payload.
    pub payload: Value,
}

/// Validate an opaque protocol identifier.
pub fn valid_protocol_id(value: &str, max_len: usize) -> bool {
    let len = value.len();
    len > 0
        && len <= max_len
        && value == value.trim()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '/'))
}

fn non_empty_bounded(value: &str, max_len: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_len
}

fn validate_scope(scope: Option<&CommandScope>) -> Result<(), RejectionReason> {
    let Some(scope) = scope else {
        return Ok(());
    };
    for value in [
        scope.conversation_id.as_deref(),
        scope.branch_id.as_deref(),
        scope.turn_id.as_deref(),
        scope.model_id.as_deref(),
        scope.document_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if !valid_protocol_id(value, MAX_PROTOCOL_ID_LEN) {
            return Err(RejectionReason::StaleScope);
        }
    }
    if scope.branch_id.is_some() && scope.conversation_id.is_none() {
        return Err(RejectionReason::StaleScope);
    }
    Ok(())
}

fn validate_scope_for_command(
    command_type: CommandType,
    scope: Option<&CommandScope>,
    payload: &ValidatedCommandPayload,
) -> Result<(), RejectionReason> {
    let Some(scope) = scope else {
        return if matches!(
            command_type,
            CommandType::RecoveryResume | CommandType::RecoveryRegenerate
        ) {
            Err(RejectionReason::StaleScope)
        } else {
            Ok(())
        };
    };

    let has_conversation_scope = scope.conversation_id.is_some() || scope.branch_id.is_some();
    let has_model_scope = scope.model_id.is_some();
    let has_document_scope = scope.document_id.is_some();

    match (command_type, payload) {
        (CommandType::RecoveryResume | CommandType::RecoveryRegenerate, _) => {
            if scope.conversation_id.is_none()
                || scope.branch_id.is_none()
                || has_model_scope
                || has_document_scope
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (CommandType::ChatSendMessage | CommandType::ChatClearConversation, _) => {
            if has_model_scope || has_document_scope {
                return Err(RejectionReason::StaleScope);
            }
        }
        (CommandType::ChatStopGeneration, _) => {
            if has_model_scope
                || has_document_scope
                || scope.conversation_id.is_none()
                || scope.branch_id.is_none()
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (CommandType::ModelDownload, ValidatedCommandPayload::ModelDownload(value)) => {
            if has_conversation_scope
                || has_document_scope
                || scope
                    .model_id
                    .as_deref()
                    .is_some_and(|id| id != value.model_id.as_str())
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (
            CommandType::ModelSelect | CommandType::ModelDelete,
            ValidatedCommandPayload::Model(value),
        ) => {
            if has_conversation_scope
                || has_document_scope
                || scope
                    .model_id
                    .as_deref()
                    .is_some_and(|id| id != value.model_id.as_str())
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (
            CommandType::DocumentRevoke | CommandType::DocumentRetryIngestion,
            ValidatedCommandPayload::Document(value),
        ) => {
            if has_conversation_scope
                || has_model_scope
                || scope
                    .document_id
                    .as_deref()
                    .is_some_and(|id| id != value.document_id.as_str())
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (
            CommandType::DocumentGrant
            | CommandType::AppInitialize
            | CommandType::DownloadCancel
            | CommandType::SettingsUpdate,
            _,
        ) if has_conversation_scope || has_model_scope || has_document_scope => {
            return Err(RejectionReason::StaleScope);
        }
        _ => {}
    }
    Ok(())
}

/// Parse and type-check one command envelope before any execution occurs.
pub fn validate_command(envelope: CommandEnvelopeV2) -> Result<ValidatedCommand, RejectionReason> {
    if !envelope.protocol_version.is_compatible() {
        return Err(RejectionReason::UnsupportedProtocol);
    }
    if !valid_protocol_id(&envelope.command_id, MAX_PROTOCOL_ID_LEN)
        || !valid_protocol_id(&envelope.request_id, MAX_PROTOCOL_ID_LEN)
        || !valid_protocol_id(&envelope.correlation_id, MAX_PROTOCOL_ID_LEN)
    {
        return Err(RejectionReason::InvalidPayload);
    }
    if let Some(operation_id) = envelope.operation_id.as_deref() {
        if !valid_protocol_id(operation_id, MAX_PROTOCOL_ID_LEN) {
            return Err(RejectionReason::InvalidPayload);
        }
    }
    if envelope.command_type.is_empty() || envelope.command_type.len() > MAX_COMMAND_TYPE_LEN {
        return Err(RejectionReason::UnknownCommand);
    }
    let command_type =
        CommandType::parse(&envelope.command_type).ok_or(RejectionReason::UnknownCommand)?;
    if command_type == CommandType::ChatStopGeneration && envelope.operation_id.is_none() {
        return Err(RejectionReason::StaleScope);
    }
    validate_scope(envelope.scope.as_ref())?;
    if command_type.requires_idempotency_key() {
        let key = envelope
            .idempotency_key
            .as_deref()
            .ok_or(RejectionReason::InvalidPayload)?;
        if !valid_protocol_id(key, MAX_IDEMPOTENCY_KEY_LEN) {
            return Err(RejectionReason::InvalidPayload);
        }
    } else if let Some(key) = envelope.idempotency_key.as_deref() {
        if !valid_protocol_id(key, MAX_IDEMPOTENCY_KEY_LEN) {
            return Err(RejectionReason::InvalidPayload);
        }
    }

    let payload = match command_type {
        CommandType::AppInitialize => {
            let value: InitializePayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !non_empty_bounded(&value.config_path, 4096) {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::Initialize(value)
        }
        CommandType::ChatSendMessage => {
            let value: SendMessagePayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !non_empty_bounded(&value.text, 64 * 1024) {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::SendMessage(value)
        }
        CommandType::ModelDownload => {
            let value: ModelDownloadPayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !non_empty_bounded(&value.model_id, 2048) || value.sha256.len() > 128 {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::ModelDownload(value)
        }
        CommandType::ModelSelect | CommandType::ModelDelete => {
            let value: ModelPayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !valid_protocol_id(&value.model_id, MAX_PROTOCOL_ID_LEN) {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::Model(value)
        }
        CommandType::DocumentGrant => {
            let value: DocumentGrantPayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !non_empty_bounded(&value.target, 8192)
                || !non_empty_bounded(&value.label, 512)
                || !non_empty_bounded(&value.mime_type, 256)
            {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::DocumentGrant(value)
        }
        CommandType::DocumentRevoke | CommandType::DocumentRetryIngestion => {
            let value: DocumentPayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !valid_protocol_id(&value.document_id, MAX_PROTOCOL_ID_LEN) {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::Document(value)
        }
        CommandType::SettingsUpdate => {
            let value: SettingUpdatePayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !non_empty_bounded(&value.key, 128)
                || !(value.value.is_boolean() || value.value.is_number() || value.value.is_string())
            {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::SettingUpdate(value)
        }
        CommandType::ChatStopGeneration
        | CommandType::ChatClearConversation
        | CommandType::DownloadCancel
        | CommandType::RecoveryResume
        | CommandType::RecoveryRegenerate => {
            if !envelope.payload.is_object()
                || envelope
                    .payload
                    .as_object()
                    .is_some_and(|value| !value.is_empty())
            {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::Empty
        }
    };

    validate_scope_for_command(command_type, envelope.scope.as_ref(), &payload)?;

    Ok(ValidatedCommand {
        envelope,
        command_type,
        payload,
    })
}
