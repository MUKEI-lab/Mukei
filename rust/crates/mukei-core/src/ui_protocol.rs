//! Versioned local protocol between the Kotlin application layer and the
//! platform-neutral Rust runtime.
//!
//! Protocol V2 is transport-neutral. Android currently carries these envelopes
//! in-process over JNI as bounded UTF-8 JSON. Domain logic remains outside this
//! module; this module owns only validation, identities, acknowledgements,
//! ordered events, batches, snapshots, and capability negotiation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Current incompatible protocol generation.
pub const PROTOCOL_MAJOR: u16 = 2;
/// Current backward-compatible protocol generation.
pub const PROTOCOL_MINOR: u16 = 3;
/// Oldest peer major accepted by this implementation.
pub const MIN_SUPPORTED_PEER_MAJOR: u16 = 2;
/// Maximum serialized command envelope accepted by a native transport.
pub const MAX_COMMAND_ENVELOPE_BYTES: usize = 64 * 1024;
/// Maximum serialized event batch returned by a native transport.
pub const MAX_EVENT_BATCH_BYTES: usize = 512 * 1024;
/// Maximum number of events returned by one drain operation.
pub const MAX_EVENT_BATCH_ITEMS: usize = 256;
/// Maximum opaque protocol identifier length.
pub const MAX_PROTOCOL_ID_LEN: usize = 128;
/// Maximum command type length.
pub const MAX_COMMAND_TYPE_LEN: usize = 96;
/// Maximum event type length.
pub const MAX_EVENT_TYPE_LEN: usize = 128;
/// Maximum idempotency key length.
pub const MAX_IDEMPOTENCY_KEY_LEN: usize = 192;

/// Capability: Protocol V2 command envelopes.
pub const CAP_COMMAND_ENVELOPE_V2: &str = "command_envelope_v2";
/// Capability: immediate accepted/rejected acknowledgements.
pub const CAP_COMMAND_ACKNOWLEDGEMENT: &str = "command_acknowledgement";
/// Capability: globally unique event identities.
pub const CAP_EVENT_IDENTITY: &str = "event_identity";
/// Capability: monotonic sequencing inside each logical stream.
pub const CAP_PER_STREAM_SEQUENCING: &str = "per_stream_sequencing";
/// Capability: bounded replay protection for idempotent commands.
pub const CAP_IDEMPOTENT_COMMAND_REPLAY: &str = "idempotent_command_replay";
/// Capability: command-correlated operation lifecycle events.
pub const CAP_OPERATION_LIFECYCLE_EVENTS: &str = "operation_lifecycle_events";
/// Capability: chat operations require explicit conversation, branch, and operation scope.
pub const CAP_SCOPED_CHAT_OPERATIONS: &str = "scoped_chat_operations";
/// Capability: event delivery through bounded drain batches.
pub const CAP_BOUNDED_EVENT_DRAIN: &str = "bounded_event_drain";
/// Capability: authoritative domain snapshots for recovery.
pub const CAP_RUNTIME_SNAPSHOTS: &str = "runtime_snapshots";
/// Capability: deterministic process-scoped runtime shutdown.
pub const CAP_GRACEFUL_SHUTDOWN: &str = "graceful_shutdown";
/// Capability implemented by the Android transport adapter.
pub const CAP_ANDROID_JNI_TRANSPORT: &str = "android_jni_transport";

/// Client family participating in protocol negotiation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    /// Native Android application written in Kotlin/Compose.
    Android,
}

/// Version carried by every V2 protocol envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    /// Incompatible generation.
    pub major: u16,
    /// Backward-compatible generation.
    pub minor: u16,
}

impl ProtocolVersion {
    /// Current local version.
    pub const CURRENT: Self = Self {
        major: PROTOCOL_MAJOR,
        minor: PROTOCOL_MINOR,
    };

    /// Whether this implementation can safely consume the version.
    pub fn is_compatible(self) -> bool {
        self.major == PROTOCOL_MAJOR && self.major >= MIN_SUPPORTED_PEER_MAJOR
    }
}

/// Protocol capability snapshot advertised by one runtime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolCapabilitySnapshot {
    /// Current local version.
    pub current_version: ProtocolVersion,
    /// Minimum accepted peer major.
    pub minimum_supported_peer_major: u16,
    /// Stable machine capability names actually implemented.
    pub capabilities: Vec<String>,
}

impl ProtocolCapabilitySnapshot {
    /// Baseline capabilities implemented by the platform-neutral runtime.
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
                CAP_BOUNDED_EVENT_DRAIN.into(),
                CAP_RUNTIME_SNAPSHOTS.into(),
                CAP_GRACEFUL_SHUTDOWN.into(),
            ],
        }
    }

    /// Build a capability snapshot that also identifies implemented commands.
    pub fn for_commands(commands: &[CommandType]) -> Self {
        let mut snapshot = Self::current();
        for command in commands {
            snapshot
                .capabilities
                .push(format!("command:{}", command.as_str()));
        }
        snapshot
    }

    /// Add one transport capability when the adapter genuinely implements it.
    pub fn with_transport(mut self, capability: &str) -> Self {
        if !self.capabilities.iter().any(|value| value == capability) {
            self.capabilities.push(capability.to_owned());
        }
        self
    }
}

/// Android runtime contract negotiated before feature commands are submitted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeContractSnapshot {
    /// Client family expected by this contract.
    pub client_kind: ClientKind,
    /// Process-scoped native runtime session identity.
    pub runtime_session_id: String,
    /// Protocol capabilities implemented by the runtime and transport.
    pub protocol: ProtocolCapabilitySnapshot,
    /// Snapshot schema generation.
    pub snapshot_schema_version: u16,
}

/// Structured logical command scope.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandScope {
    /// Optional conversation identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// Optional branch identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_id: Option<String>,
    /// Optional turn identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// Optional model identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// Optional document identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
}

/// Serialized command accepted by the native runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandEnvelopeV2 {
    /// Protocol version.
    pub protocol_version: ProtocolVersion,
    /// Stable command identity.
    pub command_id: String,
    /// Identity of this submission attempt.
    pub request_id: String,
    /// Stable machine command type.
    pub command_type: String,
    /// Client submission time.
    pub submitted_at: DateTime<Utc>,
    /// Existing or proposed operation identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// Identity shared by acknowledgement and resulting events.
    pub correlation_id: String,
    /// Replay-protection key when supported by the command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Optional logical scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<CommandScope>,
    /// Structured command payload.
    pub payload: Value,
}

/// Canonical command registry crossing the application/runtime boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandType {
    /// Initialize the native runtime.
    AppInitialize,
    /// Submit a chat message.
    ChatSendMessage,
    /// Cancel an active generation.
    ChatStopGeneration,
    /// Clear messages from the active conversation branch without deleting its identity.
    ChatClearConversation,
    /// Rename one durable conversation.
    ConversationRename,
    /// Archive one durable conversation and make it read-only.
    ConversationArchive,
    /// Permanently delete one durable conversation and all of its branches.
    ConversationDelete,
    /// Persist the active branch selected for one conversation.
    ConversationSelectBranch,
    /// Start a model download.
    ModelDownload,
    /// Cancel model downloads.
    DownloadCancel,
    /// Select an installed model.
    ModelSelect,
    /// Delete an installed model.
    ModelDelete,
    /// Grant access to a staged private document.
    DocumentGrant,
    /// Revoke a private document.
    DocumentRevoke,
    /// Retry document ingestion.
    DocumentRetryIngestion,
    /// Import a selected Android document into the active chat workspace.
    StorageImportFile,
    /// Create a durable encrypted project record.
    ProjectCreate,
    /// Update an active project record.
    ProjectUpdate,
    /// Archive a project without deleting its durable identity.
    ProjectArchive,
    /// Replace persistent instructions owned by one active project.
    ProjectInstructionsUpdate,
    /// Add one isolated memory entry to an active project.
    ProjectMemoryAdd,
    /// Update one isolated memory entry inside its owning project.
    ProjectMemoryUpdate,
    /// Delete one isolated memory entry inside its owning project.
    ProjectMemoryDelete,
    /// Persist one product setting.
    SettingsUpdate,
    /// Resume an interrupted response.
    RecoveryResume,
    /// Regenerate an interrupted response.
    RecoveryRegenerate,
}

impl CommandType {
    /// Parse a stable serialized command type.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "app.initialize" => Some(Self::AppInitialize),
            "chat.send_message" => Some(Self::ChatSendMessage),
            "chat.stop_generation" => Some(Self::ChatStopGeneration),
            "chat.clear_conversation" => Some(Self::ChatClearConversation),
            "conversation.rename" => Some(Self::ConversationRename),
            "conversation.archive" => Some(Self::ConversationArchive),
            "conversation.delete" => Some(Self::ConversationDelete),
            "conversation.select_branch" => Some(Self::ConversationSelectBranch),
            "model.download" => Some(Self::ModelDownload),
            "download.cancel" => Some(Self::DownloadCancel),
            "model.select" => Some(Self::ModelSelect),
            "model.delete" => Some(Self::ModelDelete),
            "document.grant" => Some(Self::DocumentGrant),
            "document.revoke" => Some(Self::DocumentRevoke),
            "document.retry_ingestion" => Some(Self::DocumentRetryIngestion),
            "storage.import_file" => Some(Self::StorageImportFile),
            "project.create" => Some(Self::ProjectCreate),
            "project.update" => Some(Self::ProjectUpdate),
            "project.archive" => Some(Self::ProjectArchive),
            "project.instructions.update" => Some(Self::ProjectInstructionsUpdate),
            "project.memory.add" => Some(Self::ProjectMemoryAdd),
            "project.memory.update" => Some(Self::ProjectMemoryUpdate),
            "project.memory.delete" => Some(Self::ProjectMemoryDelete),
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
            Self::ConversationRename => "conversation.rename",
            Self::ConversationArchive => "conversation.archive",
            Self::ConversationDelete => "conversation.delete",
            Self::ConversationSelectBranch => "conversation.select_branch",
            Self::ModelDownload => "model.download",
            Self::DownloadCancel => "download.cancel",
            Self::ModelSelect => "model.select",
            Self::ModelDelete => "model.delete",
            Self::DocumentGrant => "document.grant",
            Self::DocumentRevoke => "document.revoke",
            Self::DocumentRetryIngestion => "document.retry_ingestion",
            Self::StorageImportFile => "storage.import_file",
            Self::ProjectCreate => "project.create",
            Self::ProjectUpdate => "project.update",
            Self::ProjectArchive => "project.archive",
            Self::ProjectInstructionsUpdate => "project.instructions.update",
            Self::ProjectMemoryAdd => "project.memory.add",
            Self::ProjectMemoryUpdate => "project.memory.update",
            Self::ProjectMemoryDelete => "project.memory.delete",
            Self::SettingsUpdate => "settings.update",
            Self::RecoveryResume => "recovery.resume",
            Self::RecoveryRegenerate => "recovery.regenerate",
        }
    }

    /// Whether a command requires an idempotency key.
    pub const fn requires_idempotency_key(self) -> bool {
        matches!(
            self,
            Self::ChatSendMessage
                | Self::ConversationRename
                | Self::ConversationArchive
                | Self::ConversationDelete
                | Self::ConversationSelectBranch
                | Self::ModelDownload
                | Self::ModelDelete
                | Self::DocumentGrant
                | Self::DocumentRevoke
                | Self::DocumentRetryIngestion
                | Self::StorageImportFile
                | Self::ProjectCreate
                | Self::ProjectUpdate
                | Self::ProjectArchive
                | Self::ProjectInstructionsUpdate
                | Self::ProjectMemoryAdd
                | Self::ProjectMemoryUpdate
                | Self::ProjectMemoryDelete
                | Self::RecoveryResume
                | Self::RecoveryRegenerate
        )
    }

    /// Whether a command participates in operation lifecycle projection.
    pub const fn creates_operation(self) -> bool {
        true
    }
}

/// Runtime initialization payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializePayload {
    /// App-private configuration path.
    pub config_path: String,
}

/// Chat submission payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendMessagePayload {
    /// User-authored text.
    pub text: String,
    /// Optional active project to bind when creating a brand-new conversation.
    #[serde(default)]
    pub project_id: Option<String>,
}

/// Mutable conversation title payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationRenamePayload {
    /// Replacement user-visible title.
    pub title: String,
}

/// Payload containing one model identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPayload {
    /// Canonical model identity.
    pub model_id: String,
}

/// Model download payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDownloadPayload {
    /// Approved model identity or HTTPS source.
    pub model_id: String,
    /// Optional pinned SHA-256 digest.
    #[serde(default)]
    pub sha256: String,
}

/// Private document grant payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentGrantPayload {
    /// Opaque app-private staged target.
    pub target: String,
    /// User-visible document label.
    pub label: String,
    /// MIME type.
    pub mime_type: String,
}

/// Android document selected for encrypted workspace import.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageImportPayload {
    /// Opaque `content://` URI handled only by the Android document port.
    pub target: String,
    /// User-visible filename validated again by storage admission policy.
    pub display_name: String,
    /// MIME type reported by Android.
    pub mime_type: String,
}

/// Payload containing one document identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentPayload {
    /// Opaque document identity.
    pub document_id: String,
}

/// New durable project payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCreatePayload {
    /// User-visible project name, trimmed and bounded by protocol validation.
    pub name: String,
    /// Optional user-authored project description.
    #[serde(default)]
    pub description: String,
}

/// Mutable project metadata payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectUpdatePayload {
    /// Stable project identity allocated by the native runtime.
    pub project_id: String,
    /// Replacement user-visible project name.
    pub name: String,
    /// Replacement optional project description.
    #[serde(default)]
    pub description: String,
}

/// Payload containing one project identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPayload {
    /// Stable project identity allocated by the native runtime.
    pub project_id: String,
}

/// Persistent instructions owned by one project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInstructionsPayload {
    /// Stable project identity allocated by the native runtime.
    pub project_id: String,
    /// Replacement instructions. Empty content explicitly clears instructions.
    pub instructions: String,
}

/// New isolated memory entry owned by one project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMemoryCreatePayload {
    /// Stable project identity that owns the memory entry.
    pub project_id: String,
    /// User-authored memory content.
    pub content: String,
}

/// Mutable isolated project-memory payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMemoryUpdatePayload {
    /// Stable project identity that owns the memory entry.
    pub project_id: String,
    /// Stable memory identity allocated inside the owning project.
    pub memory_id: String,
    /// Replacement user-authored memory content.
    pub content: String,
}

/// Identity pair for one memory entry inside one project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMemoryPayload {
    /// Stable project identity that owns the memory entry.
    pub project_id: String,
    /// Stable memory identity that must resolve inside that project only.
    pub memory_id: String,
}

/// Product setting mutation payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingUpdatePayload {
    /// Stable setting key.
    pub key: String,
    /// JSON scalar setting value.
    pub value: Value,
}

/// Type-checked command payload.
#[derive(Clone, Debug, PartialEq)]
pub enum ValidatedCommandPayload {
    /// No payload fields.
    Empty,
    /// Runtime initialization.
    Initialize(InitializePayload),
    /// Chat submission.
    SendMessage(SendMessagePayload),
    /// Conversation title mutation.
    ConversationRename(ConversationRenamePayload),
    /// Model download.
    ModelDownload(ModelDownloadPayload),
    /// Model identity.
    Model(ModelPayload),
    /// Private document grant.
    DocumentGrant(DocumentGrantPayload),
    /// Document identity.
    Document(DocumentPayload),
    /// Selected document destined for encrypted workspace storage.
    StorageImport(StorageImportPayload),
    /// New project metadata.
    ProjectCreate(ProjectCreatePayload),
    /// Updated project metadata.
    ProjectUpdate(ProjectUpdatePayload),
    /// Existing project identity.
    Project(ProjectPayload),
    /// Replacement project instructions.
    ProjectInstructions(ProjectInstructionsPayload),
    /// New project-memory content.
    ProjectMemoryCreate(ProjectMemoryCreatePayload),
    /// Updated project-memory content.
    ProjectMemoryUpdate(ProjectMemoryUpdatePayload),
    /// Existing project-memory identity pair.
    ProjectMemory(ProjectMemoryPayload),
    /// Setting mutation.
    SettingUpdate(SettingUpdatePayload),
}

/// Structurally validated command ready for policy and dispatch.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedCommand {
    /// Original envelope.
    pub envelope: CommandEnvelopeV2,
    /// Parsed command registry value.
    pub command_type: CommandType,
    /// Type-checked payload.
    pub payload: ValidatedCommandPayload,
}

/// Acknowledgement status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcknowledgementStatus {
    /// Validated and accepted for processing.
    Accepted,
    /// Rejected before execution.
    Rejected,
}

/// Stable command rejection reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    /// Protocol major is unsupported.
    UnsupportedProtocol,
    /// Command type is unknown.
    UnknownCommand,
    /// Envelope or typed payload is malformed.
    InvalidPayload,
    /// Required capability is unavailable.
    CapabilityUnavailable,
    /// Conflicting operation prevents acceptance.
    BusyConflict,
    /// Logical scope is stale or invalid.
    StaleScope,
    /// Native backend is unavailable.
    BackendUnavailable,
    /// Idempotency key conflicts with different content.
    DuplicateReplayConflict,
    /// Local policy denied the command.
    PolicyDenied,
}

/// Immediate command acknowledgement.
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
    /// Allocated or targeted operation identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// Accepted or rejected.
    pub status: AcknowledgementStatus,
    /// Rejection reason when rejected.
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

    /// Build a rejected acknowledgement.
    pub fn rejected(envelope: Option<&CommandEnvelopeV2>, reason: RejectionReason) -> Self {
        Self {
            protocol_version: ProtocolVersion::CURRENT,
            command_id: envelope
                .map(|value| value.command_id.clone())
                .unwrap_or_default(),
            request_id: envelope
                .map(|value| value.request_id.clone())
                .unwrap_or_default(),
            correlation_id: envelope
                .map(|value| value.correlation_id.clone())
                .unwrap_or_default(),
            operation_id: None,
            status: AcknowledgementStatus::Rejected,
            rejection_reason: Some(reason),
            timestamp: Utc::now(),
        }
    }

    /// Validate correlation and status invariants for one command.
    pub fn validate_for(&self, command: &CommandEnvelopeV2) -> bool {
        if !self.protocol_version.is_compatible()
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

/// Reliable ordered event emitted by the native runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelopeV2 {
    /// Protocol version.
    pub protocol_version: ProtocolVersion,
    /// Globally unique event identity.
    pub event_id: String,
    /// Ordered logical stream identity.
    pub stream_id: String,
    /// Monotonic sequence within the stream.
    pub sequence: u64,
    /// Stable event type.
    pub event_type: String,
    /// Emission timestamp.
    pub emitted_at: DateTime<Utc>,
    /// Correlation identity when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Operation identity when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// Request identity when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Command identity when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    /// Originating command type when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_type: Option<String>,
    /// Structured event payload.
    pub payload: Value,
}

impl EventEnvelopeV2 {
    /// Validate event envelope invariants before projection into Kotlin state.
    pub fn validate(&self) -> bool {
        self.protocol_version.is_compatible()
            && valid_protocol_id(&self.event_id, MAX_PROTOCOL_ID_LEN)
            && valid_protocol_id(&self.stream_id, MAX_PROTOCOL_ID_LEN)
            && self.sequence > 0
            && non_empty_bounded(&self.event_type, MAX_EVENT_TYPE_LEN)
            && optional_protocol_id_valid(self.correlation_id.as_deref())
            && optional_protocol_id_valid(self.operation_id.as_deref())
            && optional_protocol_id_valid(self.request_id.as_deref())
            && optional_protocol_id_valid(self.command_id.as_deref())
            && self
                .command_type
                .as_deref()
                .is_none_or(|value| non_empty_bounded(value, MAX_COMMAND_TYPE_LEN))
    }
}

/// One bounded event-drain response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventBatchV2 {
    /// Protocol version.
    pub protocol_version: ProtocolVersion,
    /// Native runtime session identity.
    pub runtime_session_id: String,
    /// Batch creation timestamp.
    pub drained_at: DateTime<Utc>,
    /// Ordered events drained from the native queue.
    pub events: Vec<EventEnvelopeV2>,
    /// Whether additional events were known to remain when the batch was built.
    pub has_more: bool,
}

impl EventBatchV2 {
    /// Validate batch and contained event invariants.
    pub fn validate(&self) -> bool {
        self.protocol_version.is_compatible()
            && valid_protocol_id(&self.runtime_session_id, MAX_PROTOCOL_ID_LEN)
            && self.events.len() <= MAX_EVENT_BATCH_ITEMS
            && self.events.iter().all(EventEnvelopeV2::validate)
    }
}

/// Authoritative snapshot domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotDomainV2 {
    /// Runtime lifecycle and session metadata.
    Application,
    /// Product settings projection.
    Settings,
    /// Protocol capability contract.
    Protocol,
    /// Operation and replay registry summary.
    Operations,
    /// Durable encrypted project records.
    Projects,
}

impl SnapshotDomainV2 {
    /// Parse a stable snapshot-domain string.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "application" => Some(Self::Application),
            "settings" => Some(Self::Settings),
            "protocol" => Some(Self::Protocol),
            "operations" => Some(Self::Operations),
            "projects" => Some(Self::Projects),
            _ => None,
        }
    }
}

/// Versioned authoritative runtime snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotEnvelopeV2 {
    /// Protocol version.
    pub protocol_version: ProtocolVersion,
    /// Native runtime session identity.
    pub runtime_session_id: String,
    /// Snapshot domain.
    pub domain: SnapshotDomainV2,
    /// Domain schema generation.
    pub schema_version: u16,
    /// Snapshot generation timestamp.
    pub generated_at: DateTime<Utc>,
    /// Domain payload.
    pub payload: Value,
}

impl SnapshotEnvelopeV2 {
    /// Validate snapshot identity and schema invariants.
    pub fn validate(&self) -> bool {
        self.protocol_version.is_compatible()
            && valid_protocol_id(&self.runtime_session_id, MAX_PROTOCOL_ID_LEN)
            && self.schema_version > 0
    }
}

/// Validate an opaque protocol identifier.
pub fn valid_protocol_id(value: &str, max_len: usize) -> bool {
    let len = value.len();
    len > 0
        && len <= max_len
        && value == value.trim()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':' | '/')
        })
}

fn optional_protocol_id_valid(value: Option<&str>) -> bool {
    value.is_none_or(|value| valid_protocol_id(value, MAX_PROTOCOL_ID_LEN))
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
            CommandType::RecoveryResume
                | CommandType::RecoveryRegenerate
                | CommandType::StorageImportFile
                | CommandType::ConversationRename
                | CommandType::ConversationArchive
                | CommandType::ConversationDelete
                | CommandType::ConversationSelectBranch
        ) {
            Err(RejectionReason::StaleScope)
        } else {
            Ok(())
        };
    };

    let has_conversation = scope.conversation_id.is_some() || scope.branch_id.is_some();
    let has_model = scope.model_id.is_some();
    let has_document = scope.document_id.is_some();

    match (command_type, payload) {
        (CommandType::RecoveryResume | CommandType::RecoveryRegenerate, _) => {
            if scope.conversation_id.is_none()
                || scope.branch_id.is_none()
                || has_model
                || has_document
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (CommandType::ChatSendMessage | CommandType::ChatClearConversation, _) => {
            if has_model || has_document {
                return Err(RejectionReason::StaleScope);
            }
        }
        (
            CommandType::ConversationRename
            | CommandType::ConversationArchive
            | CommandType::ConversationDelete,
            _,
        ) => {
            if scope.conversation_id.is_none()
                || scope.branch_id.is_some()
                || scope.turn_id.is_some()
                || has_model
                || has_document
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (CommandType::ConversationSelectBranch, _) => {
            if scope.conversation_id.is_none()
                || scope.branch_id.is_none()
                || scope.turn_id.is_some()
                || has_model
                || has_document
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (CommandType::ChatStopGeneration, _) => {
            if has_model
                || has_document
                || scope.conversation_id.is_none()
                || scope.branch_id.is_none()
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (CommandType::ModelDownload, ValidatedCommandPayload::ModelDownload(value)) => {
            if has_conversation
                || has_document
                || scope
                    .model_id
                    .as_deref()
                    .is_some_and(|identity| identity != value.model_id)
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (
            CommandType::ModelSelect | CommandType::ModelDelete,
            ValidatedCommandPayload::Model(value),
        ) => {
            if has_conversation
                || has_document
                || scope
                    .model_id
                    .as_deref()
                    .is_some_and(|identity| identity != value.model_id)
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (CommandType::StorageImportFile, ValidatedCommandPayload::StorageImport(_)) => {
            if scope.conversation_id.is_none() || has_model || has_document {
                return Err(RejectionReason::StaleScope);
            }
        }
        (
            CommandType::DocumentRevoke | CommandType::DocumentRetryIngestion,
            ValidatedCommandPayload::Document(value),
        ) => {
            if has_conversation
                || has_model
                || scope
                    .document_id
                    .as_deref()
                    .is_some_and(|identity| identity != value.document_id)
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (
            CommandType::DocumentGrant
            | CommandType::AppInitialize
            | CommandType::DownloadCancel
            | CommandType::ProjectCreate
            | CommandType::ProjectUpdate
            | CommandType::ProjectArchive
            | CommandType::ProjectInstructionsUpdate
            | CommandType::ProjectMemoryAdd
            | CommandType::ProjectMemoryUpdate
            | CommandType::ProjectMemoryDelete
            | CommandType::SettingsUpdate,
            _,
        ) if has_conversation || has_model || has_document => {
            return Err(RejectionReason::StaleScope);
        }
        _ => {}
    }
    Ok(())
}

/// Parse and type-check a command before execution.
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
    if !optional_protocol_id_valid(envelope.operation_id.as_deref()) {
        return Err(RejectionReason::InvalidPayload);
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
    } else if envelope
        .idempotency_key
        .as_deref()
        .is_some_and(|key| !valid_protocol_id(key, MAX_IDEMPOTENCY_KEY_LEN))
    {
        return Err(RejectionReason::InvalidPayload);
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
            if !non_empty_bounded(&value.text, 64 * 1024)
                || value
                    .project_id
                    .as_deref()
                    .is_some_and(|project_id| !valid_protocol_id(project_id, MAX_PROTOCOL_ID_LEN))
            {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::SendMessage(value)
        }
        CommandType::ConversationRename => {
            let value: ConversationRenamePayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !non_empty_bounded(&value.title, 128) || value.title.chars().any(char::is_control) {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::ConversationRename(value)
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
        CommandType::StorageImportFile => {
            let value: StorageImportPayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !non_empty_bounded(&value.target, 8192)
                || !value.target.starts_with("content://")
                || value.target.chars().any(char::is_control)
                || !non_empty_bounded(&value.display_name, 512)
                || !non_empty_bounded(&value.mime_type, 256)
            {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::StorageImport(value)
        }
        CommandType::DocumentRevoke | CommandType::DocumentRetryIngestion => {
            let value: DocumentPayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !valid_protocol_id(&value.document_id, MAX_PROTOCOL_ID_LEN) {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::Document(value)
        }
        CommandType::ProjectCreate => {
            let value: ProjectCreatePayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !non_empty_bounded(&value.name, 128)
                || value.name.chars().any(char::is_control)
                || value.description.len() > 4 * 1024
                || value.description.chars().any(|character| character == '\0')
            {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::ProjectCreate(value)
        }
        CommandType::ProjectUpdate => {
            let value: ProjectUpdatePayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !valid_protocol_id(&value.project_id, MAX_PROTOCOL_ID_LEN)
                || !non_empty_bounded(&value.name, 128)
                || value.name.chars().any(char::is_control)
                || value.description.len() > 4 * 1024
                || value.description.chars().any(|character| character == '\0')
            {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::ProjectUpdate(value)
        }
        CommandType::ProjectArchive => {
            let value: ProjectPayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !valid_protocol_id(&value.project_id, MAX_PROTOCOL_ID_LEN) {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::Project(value)
        }
        CommandType::ProjectInstructionsUpdate => {
            let value: ProjectInstructionsPayload =
                serde_json::from_value(envelope.payload.clone())
                    .map_err(|_| RejectionReason::InvalidPayload)?;
            if !valid_protocol_id(&value.project_id, MAX_PROTOCOL_ID_LEN)
                || value.instructions.len() > 4 * 1024
                || value.instructions.chars().any(|character| character == ' ')
            {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::ProjectInstructions(value)
        }
        CommandType::ProjectMemoryAdd => {
            let value: ProjectMemoryCreatePayload =
                serde_json::from_value(envelope.payload.clone())
                    .map_err(|_| RejectionReason::InvalidPayload)?;
            if !valid_protocol_id(&value.project_id, MAX_PROTOCOL_ID_LEN)
                || !non_empty_bounded(&value.content, 1024)
                || value.content.chars().any(|character| character == ' ')
            {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::ProjectMemoryCreate(value)
        }
        CommandType::ProjectMemoryUpdate => {
            let value: ProjectMemoryUpdatePayload =
                serde_json::from_value(envelope.payload.clone())
                    .map_err(|_| RejectionReason::InvalidPayload)?;
            if !valid_protocol_id(&value.project_id, MAX_PROTOCOL_ID_LEN)
                || !valid_protocol_id(&value.memory_id, MAX_PROTOCOL_ID_LEN)
                || !non_empty_bounded(&value.content, 1024)
                || value.content.chars().any(|character| character == ' ')
            {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::ProjectMemoryUpdate(value)
        }
        CommandType::ProjectMemoryDelete => {
            let value: ProjectMemoryPayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !valid_protocol_id(&value.project_id, MAX_PROTOCOL_ID_LEN)
                || !valid_protocol_id(&value.memory_id, MAX_PROTOCOL_ID_LEN)
            {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::ProjectMemory(value)
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
        | CommandType::ConversationArchive
        | CommandType::ConversationDelete
        | CommandType::ConversationSelectBranch
        | CommandType::DownloadCancel
        | CommandType::RecoveryResume
        | CommandType::RecoveryRegenerate => {
            if !envelope.payload.is_object()
                || envelope
                    .payload
                    .as_object()
                    .is_some_and(|object| !object.is_empty())
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn initialize_command() -> CommandEnvelopeV2 {
        CommandEnvelopeV2 {
            protocol_version: ProtocolVersion::CURRENT,
            command_id: "cmd-init".into(),
            request_id: "req-init".into(),
            command_type: "app.initialize".into(),
            submitted_at: Utc::now(),
            operation_id: None,
            correlation_id: "corr-init".into(),
            idempotency_key: None,
            scope: None,
            payload: json!({ "config_path": "/data/user/0/ai.mukei.android/files/config.toml" }),
        }
    }

    #[test]
    fn android_runtime_capabilities_exclude_legacy_transport() {
        let snapshot = ProtocolCapabilitySnapshot::for_commands(&[CommandType::AppInitialize])
            .with_transport(CAP_ANDROID_JNI_TRANSPORT);
        assert!(snapshot
            .capabilities
            .contains(&CAP_ANDROID_JNI_TRANSPORT.to_owned()));
        assert!(!snapshot
            .capabilities
            .iter()
            .any(|value| value.contains("legacy")
                || value.contains("qml")
                || value.contains("qt")));
    }

    #[test]
    fn initialize_command_validates() {
        assert!(validate_command(initialize_command()).is_ok());
    }

    #[test]
    fn event_batch_rejects_zero_sequence() {
        let batch = EventBatchV2 {
            protocol_version: ProtocolVersion::CURRENT,
            runtime_session_id: "runtime-1".into(),
            drained_at: Utc::now(),
            events: vec![EventEnvelopeV2 {
                protocol_version: ProtocolVersion::CURRENT,
                event_id: "event-1".into(),
                stream_id: "application:lifecycle".into(),
                sequence: 0,
                event_type: "application.ready".into(),
                emitted_at: Utc::now(),
                correlation_id: None,
                operation_id: None,
                request_id: None,
                command_id: None,
                command_type: None,
                payload: json!({}),
            }],
            has_more: false,
        };
        assert!(!batch.validate());
    }
}
