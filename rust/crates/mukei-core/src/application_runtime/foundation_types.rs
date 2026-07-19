// Platform-neutral application runtime owned by the native process.
//
// Protocol V2 commands are validated once, routed to feature handlers, tracked
// as cancellable operations, projected through ordered events, and exposed via
// authoritative snapshots. Android-only services are accessed through the
// pull-based platform broker in `crate::platform`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{
    sync_channel, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError,
};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::agent::context::{ContextBackend, TokenCount};
use crate::agent::{
    AgentLoop, AgentRunRequest, ContextBudgetManager, FailureTracker, ToolExecutionPolicy,
    ToolExecutor, Watchdog, WatchdogHandle,
};
use crate::config::MukeiConfig;
use crate::engine::{
    ActivationCommit, InferenceBackendFactory, ModelActivationService, VerifiedModelArtifact,
    VerifiedModelDescriptor,
};
use crate::error::MukeiError;
use crate::platform::{
    PlatformBrokerSnapshot, PlatformPortError, PlatformRequestBatch, PlatformRequestBroker,
    PlatformRequestKind, PlatformResponse,
};
use crate::tools::ToolRegistry;
use crate::types::{BranchId, ChatMessage, ConversationId, MessageId, Role};
use crate::ui_protocol::{
    validate_command, AcknowledgementStatus, CommandAcknowledgementV2, CommandEnvelopeV2,
    CommandType, EventEnvelopeV2, ProtocolCapabilitySnapshot, RejectionReason, ValidatedCommand,
    ValidatedCommandPayload,
};

const DEFAULT_EVENT_CAPACITY: usize = 512;
const MAX_EVENT_CAPACITY: usize = 4096;
const MAX_DRAIN_BATCH: usize = 256;
const PLATFORM_WAIT_TIMEOUT: Duration = Duration::from_secs(120);
const CAP_EVENT_GAP_REPORTING: &str = "event_gap_reporting";
const CAP_PLATFORM_REQUEST_BROKER: &str = "platform_request_broker";
const CAP_ANDROID_DOCUMENT_PORT: &str = "android_document_port";
const CAP_ANDROID_KEYSTORE_PORT: &str = "android_keystore_port";

/// Configuration required to allocate one native runtime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// App-private data directory supplied by the platform layer.
    pub app_data_dir: String,
    /// Number of Tokio worker threads.
    #[serde(default = "default_worker_threads")]
    pub worker_threads: usize,
    /// Maximum blocking threads used by storage and model work.
    #[serde(default = "default_blocking_threads")]
    pub max_blocking_threads: usize,
    /// Capacity of the bounded application event queue.
    #[serde(default = "default_event_capacity")]
    pub event_capacity: usize,
}

fn default_worker_threads() -> usize {
    2
}

fn default_blocking_threads() -> usize {
    6
}

fn default_event_capacity() -> usize {
    DEFAULT_EVENT_CAPACITY
}

impl RuntimeConfig {
    fn validate(&self) -> Result<(), RuntimeError> {
        if self.app_data_dir.trim().is_empty() || self.app_data_dir.len() > 4096 {
            return Err(RuntimeError::InvalidConfig("invalid app_data_dir"));
        }
        if !(1..=8).contains(&self.worker_threads) {
            return Err(RuntimeError::InvalidConfig("worker_threads must be 1..=8"));
        }
        if !(1..=32).contains(&self.max_blocking_threads) {
            return Err(RuntimeError::InvalidConfig(
                "max_blocking_threads must be 1..=32",
            ));
        }
        if !(32..=MAX_EVENT_CAPACITY).contains(&self.event_capacity) {
            return Err(RuntimeError::InvalidConfig(
                "event_capacity must be 32..=4096",
            ));
        }
        Ok(())
    }
}

/// Optional native services installed by the composition root.
#[derive(Clone, Default)]
pub struct RuntimeServices {
    /// Production inference backend factory. Absence is represented truthfully
    /// and causes model activation/chat commands to fail closed.
    pub backend_factory: Option<Arc<dyn InferenceBackendFactory>>,
}

/// Runtime lifecycle state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Created,
    Initializing,
    Ready,
    Stopping,
    Stopped,
    Failed,
}

/// Supported snapshot domains.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSnapshotDomain {
    Application,
    Settings,
    Protocol,
    Operations,
}

impl RuntimeSnapshotDomain {
    /// Parse a stable snapshot-domain identifier.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "application" => Some(Self::Application),
            "settings" => Some(Self::Settings),
            "protocol" => Some(Self::Protocol),
            "operations" => Some(Self::Operations),
            _ => None,
        }
    }
}

/// Versioned runtime snapshot returned through a transport adapter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSnapshotEnvelope {
    pub runtime_session_id: String,
    pub domain: RuntimeSnapshotDomain,
    pub schema_version: u16,
    pub generated_at: DateTime<Utc>,
    pub payload: Value,
}

/// One bounded event-drain result.
#[derive(Clone, Debug, PartialEq)]
pub struct EventDrain {
    pub events: Vec<EventEnvelopeV2>,
    pub has_more: bool,
}

/// Runtime construction and lifecycle failures.
#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("invalid runtime configuration: {0}")]
    InvalidConfig(&'static str),
    #[error("failed to create native async runtime: {0}")]
    AsyncRuntime(#[from] std::io::Error),
    #[error("native runtime is stopped")]
    Stopped,
    #[error("unsupported snapshot domain")]
    UnsupportedSnapshot,
}

#[derive(Clone)]
struct ReplayRecord {
    fingerprint: Vec<u8>,
    acknowledgement: CommandAcknowledgementV2,
}

struct EventBus {
    sender: SyncSender<EventEnvelopeV2>,
    receiver: Mutex<Receiver<EventEnvelopeV2>>,
    queue_lock: Mutex<()>,
    suppressed_streams: Mutex<HashSet<String>>,
    suppressed_operations: Mutex<HashSet<String>>,
    sequences: Mutex<HashMap<String, u64>>,
    queued: AtomicUsize,
    dropped: AtomicU64,
}

impl EventBus {
    fn new(capacity: usize) -> Self {
        let (sender, receiver) = sync_channel(capacity);
        Self {
            sender,
            receiver: Mutex::new(receiver),
            queue_lock: Mutex::new(()),
            suppressed_streams: Mutex::new(HashSet::new()),
            suppressed_operations: Mutex::new(HashSet::new()),
            sequences: Mutex::new(HashMap::new()),
            queued: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    fn next_sequence(&self, stream_id: &str) -> u64 {
        let mut sequences = self
            .sequences
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let sequence = sequences.entry(stream_id.to_owned()).or_insert(0);
        *sequence = sequence.saturating_add(1);
        *sequence
    }

    fn build_event(
        &self,
        stream_id: &str,
        event_type: &str,
        payload: Value,
        command: Option<&CommandEnvelopeV2>,
        operation_id: Option<String>,
    ) -> EventEnvelopeV2 {
        EventEnvelopeV2 {
            protocol_version: crate::ui_protocol::ProtocolVersion::CURRENT,
            event_id: Uuid::new_v4().to_string(),
            stream_id: stream_id.to_owned(),
            sequence: self.next_sequence(stream_id),
            event_type: event_type.to_owned(),
            emitted_at: Utc::now(),
            correlation_id: command.map(|value| value.correlation_id.clone()),
            operation_id,
            request_id: command.map(|value| value.request_id.clone()),
            command_id: command.map(|value| value.command_id.clone()),
            command_type: command.map(|value| value.command_type.clone()),
            payload,
        }
    }

    fn emit(
        &self,
        stream_id: &str,
        event_type: &str,
        payload: Value,
        command: Option<&CommandEnvelopeV2>,
        operation_id: Option<String>,
    ) {
        let _queue = self
            .queue_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let stream_suppressed = self
            .suppressed_streams
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(stream_id);
        let operation_suppressed = operation_id
            .as_ref()
            .map(|operation_id| {
                self.suppressed_operations
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .contains(operation_id)
            })
            .unwrap_or(false);
        if stream_suppressed || operation_suppressed {
            return;
        }

        let event = self.build_event(stream_id, event_type, payload, command, operation_id);
        match self.sender.try_send(event) {
            Ok(()) => {
                self.queued.fetch_add(1, Ordering::Release);
            }
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Purge queued events scoped to a Temporary Chat while preserving unrelated
    /// events in original order. The same streams/operation IDs are tombstoned so
    /// producers that raced with session shutdown cannot enqueue content afterwards.
    fn purge_temporary_chat(&self, conversation_id: &str, operation_ids: &[String]) -> usize {
        let conversation_stream = format!("conversation:{conversation_id}");
        let operation_set = operation_ids.iter().cloned().collect::<HashSet<_>>();
        let operation_streams = operation_ids
            .iter()
            .map(|operation_id| format!("operation:{operation_id}"))
            .collect::<HashSet<_>>();

        let _queue = self
            .queue_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        {
            let mut suppressed_streams = self
                .suppressed_streams
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            suppressed_streams.insert(conversation_stream.clone());
            suppressed_streams.extend(operation_streams.iter().cloned());
        }
        self.suppressed_operations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extend(operation_set.iter().cloned());

        let receiver = self
            .receiver
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut retained = Vec::new();
        let mut removed = 0usize;
        loop {
            match receiver.try_recv() {
                Ok(event) => {
                    self.decrement_queued();
                    let sensitive = event.stream_id == conversation_stream
                        || operation_streams.contains(&event.stream_id)
                        || event
                            .operation_id
                            .as_ref()
                            .map(|operation_id| operation_set.contains(operation_id))
                            .unwrap_or(false);
                    if sensitive {
                        removed = removed.saturating_add(1);
                    } else {
                        retained.push(event);
                    }
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }

        for event in retained {
            match self.sender.try_send(event) {
                Ok(()) => {
                    self.queued.fetch_add(1, Ordering::Release);
                }
                Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                    // This should be unreachable while queue_lock + receiver are held,
                    // because retained <= the number just drained. Fail observably if
                    // channel semantics ever change.
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        let mut sequences = self
            .sequences
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sequences.remove(&conversation_stream);
        for stream_id in operation_streams {
            sequences.remove(&stream_id);
        }
        removed
    }

    fn decrement_queued(&self) {
        let _ = self.queued.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |value| Some(value.saturating_sub(1)),
        );
    }

    fn drain(&self, limit: usize, timeout: Duration) -> EventDrain {
        let limit = limit.clamp(1, MAX_DRAIN_BATCH);
        let mut events = Vec::with_capacity(limit);
        let dropped = self.dropped.swap(0, Ordering::AcqRel);
        if dropped > 0 {
            events.push(self.build_event(
                "application:events",
                "runtime.event_gap",
                json!({
                    "dropped_events": dropped,
                    "recovery": "request_authoritative_snapshots",
                }),
                None,
                None,
            ));
        }
        if events.len() < limit {
            let receiver = self
                .receiver
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let first = if timeout.is_zero() {
                match receiver.try_recv() {
                    Ok(event) => Some(event),
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
                }
            } else {
                match receiver.recv_timeout(timeout) {
                    Ok(event) => Some(event),
                    Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => None,
                }
            };
            if let Some(event) = first {
                self.decrement_queued();
                events.push(event);
            }
            while events.len() < limit {
                match receiver.try_recv() {
                    Ok(event) => {
                        self.decrement_queued();
                        events.push(event);
                    }
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
            }
        }
        EventDrain {
            events,
            has_more: self.queued.load(Ordering::Acquire) > 0
                || self.dropped.load(Ordering::Acquire) > 0,
        }
    }
}
