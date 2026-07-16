//! Platform-neutral application runtime owned by the native process.
//!
//! Transport adapters submit typed Protocol V2 commands, drain bounded event
//! batches, request authoritative snapshots, and trigger deterministic shutdown.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{
    sync_channel, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError,
};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::runtime::{Builder, Runtime};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::ui_protocol::{
    validate_command, AcknowledgementStatus, CommandAcknowledgementV2, CommandEnvelopeV2,
    CommandType, EventEnvelopeV2, ProtocolCapabilitySnapshot, RejectionReason, ValidatedCommand,
    ValidatedCommandPayload,
};

const DEFAULT_EVENT_CAPACITY: usize = 512;
const MAX_EVENT_CAPACITY: usize = 4096;
const MAX_DRAIN_BATCH: usize = 256;
const CAP_EVENT_GAP_REPORTING: &str = "event_gap_reporting";

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

/// Runtime lifecycle state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    /// Allocated but not initialized.
    Created,
    /// Initialization command is being processed.
    Initializing,
    /// Runtime can accept implemented domain commands.
    Ready,
    /// Shutdown has started.
    Stopping,
    /// Shutdown completed.
    Stopped,
    /// Runtime encountered a fatal internal error.
    Failed,
}

/// Supported snapshot domains.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSnapshotDomain {
    /// Runtime lifecycle and session metadata.
    Application,
    /// Current product settings projection.
    Settings,
    /// Protocol capability contract.
    Protocol,
    /// Operation and replay registry summary.
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
    /// Native runtime session identity.
    pub runtime_session_id: String,
    /// Snapshot domain.
    pub domain: RuntimeSnapshotDomain,
    /// Domain schema version.
    pub schema_version: u16,
    /// Snapshot generation time.
    pub generated_at: chrono::DateTime<Utc>,
    /// Domain payload.
    pub payload: Value,
}

/// One bounded event-drain result.
#[derive(Clone, Debug, PartialEq)]
pub struct EventDrain {
    /// Ordered events returned to the transport.
    pub events: Vec<EventEnvelopeV2>,
    /// Whether more queued or gap-report events remain.
    pub has_more: bool,
}

/// Runtime construction and lifecycle failures.
#[derive(Error, Debug)]
pub enum RuntimeError {
    /// Configuration is invalid.
    #[error("invalid runtime configuration: {0}")]
    InvalidConfig(&'static str),
    /// Tokio runtime allocation failed.
    #[error("failed to create native async runtime: {0}")]
    AsyncRuntime(#[from] std::io::Error),
    /// Runtime has already stopped.
    #[error("native runtime is stopped")]
    Stopped,
    /// Snapshot domain is unsupported.
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

struct CommandRouter;

impl CommandRouter {
    fn dispatch(
        runtime: &MukeiRuntime,
        command: &ValidatedCommand,
    ) -> CommandAcknowledgementV2 {
        match command.command_type {
            CommandType::AppInitialize => runtime.initialize(command),
            CommandType::SettingsUpdate => runtime.update_setting(command),
            _ => CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::CapabilityUnavailable,
            ),
        }
    }
}

/// Process-scoped Mukei application runtime.
pub struct MukeiRuntime {
    session_id: String,
    config: RuntimeConfig,
    state: RwLock<RuntimeState>,
    async_runtime: Runtime,
    cancellation: CancellationToken,
    events: Arc<EventBus>,
    settings: RwLock<HashMap<String, Value>>,
    replay: Mutex<HashMap<String, ReplayRecord>>,
    closed: AtomicBool,
}

impl MukeiRuntime {
    /// Allocate a native runtime with bounded resources.
    pub fn create(config: RuntimeConfig) -> Result<Self, RuntimeError> {
        config.validate()?;
        let async_runtime = Builder::new_multi_thread()
            .worker_threads(config.worker_threads)
            .max_blocking_threads(config.max_blocking_threads)
            .thread_name("mukei-native")
            .enable_all()
            .build()?;
        let events = Arc::new(EventBus::new(config.event_capacity));
        let runtime = Self {
            session_id: Uuid::new_v4().to_string(),
            config,
            state: RwLock::new(RuntimeState::Created),
            async_runtime,
            cancellation: CancellationToken::new(),
            events,
            settings: RwLock::new(HashMap::new()),
            replay: Mutex::new(HashMap::new()),
            closed: AtomicBool::new(false),
        };
        runtime.events.emit(
            "application:lifecycle",
            "runtime.created",
            json!({ "runtime_session_id": runtime.session_id }),
            None,
            None,
        );
        Ok(runtime)
    }

    /// Native runtime session identity.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Current lifecycle state.
    pub fn state(&self) -> RuntimeState {
        *self
            .state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Protocol capabilities genuinely implemented by this runtime.
    pub fn capabilities(&self) -> ProtocolCapabilitySnapshot {
        ProtocolCapabilitySnapshot::for_commands(&[
            CommandType::AppInitialize,
            CommandType::SettingsUpdate,
        ])
        .with_transport(CAP_EVENT_GAP_REPORTING)
    }

    /// Validate and submit one command.
    pub fn submit(&self, envelope: CommandEnvelopeV2) -> CommandAcknowledgementV2 {
        if self.closed.load(Ordering::Acquire) {
            return CommandAcknowledgementV2::rejected(
                Some(&envelope),
                RejectionReason::BackendUnavailable,
            );
        }
        let validated = match validate_command(envelope.clone()) {
            Ok(value) => value,
            Err(reason) => return CommandAcknowledgementV2::rejected(Some(&envelope), reason),
        };
        if let Some(acknowledgement) = self.replay_lookup(&validated) {
            return acknowledgement;
        }
        let acknowledgement = CommandRouter::dispatch(self, &validated);
        self.remember_replay(&validated, &acknowledgement);
        acknowledgement
    }

    fn initialize(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        {
            let mut state = self
                .state
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if *state == RuntimeState::Ready {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::BusyConflict,
                );
            }
            if matches!(*state, RuntimeState::Stopping | RuntimeState::Stopped) {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::BackendUnavailable,
                );
            }
            *state = RuntimeState::Initializing;
        }

        let operation_id = Uuid::new_v4().to_string();
        let acknowledgement =
            CommandAcknowledgementV2::accepted(&command.envelope, Some(operation_id.clone()));
        self.events.emit(
            &format!("operation:{operation_id}"),
            "operation.accepted",
            json!({ "state": "accepted" }),
            Some(&command.envelope),
            Some(operation_id.clone()),
        );
        *self
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = RuntimeState::Ready;
        self.events.emit(
            "application:lifecycle",
            "application.ready",
            json!({
                "runtime_session_id": self.session_id,
                "app_data_dir": self.config.app_data_dir,
            }),
            Some(&command.envelope),
            Some(operation_id.clone()),
        );
        self.events.emit(
            &format!("operation:{operation_id}"),
            "operation.completed",
            json!({ "state": "completed" }),
            Some(&command.envelope),
            Some(operation_id),
        );
        acknowledgement
    }

    fn update_setting(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if self.state() != RuntimeState::Ready {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::BackendUnavailable,
            );
        }
        let ValidatedCommandPayload::SettingUpdate(setting) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        self.settings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(setting.key.clone(), setting.value.clone());

        let operation_id = Uuid::new_v4().to_string();
        let acknowledgement =
            CommandAcknowledgementV2::accepted(&command.envelope, Some(operation_id.clone()));
        self.events.emit(
            "application:settings",
            "settings.updated",
            json!({ "key": setting.key, "value": setting.value }),
            Some(&command.envelope),
            Some(operation_id.clone()),
        );
        self.events.emit(
            &format!("operation:{operation_id}"),
            "operation.completed",
            json!({ "state": "completed" }),
            Some(&command.envelope),
            Some(operation_id),
        );
        acknowledgement
    }

    fn fingerprint(command: &ValidatedCommand) -> Option<Vec<u8>> {
        serde_json::to_vec(&json!({
            "command_type": command.envelope.command_type,
            "operation_id": command.envelope.operation_id,
            "scope": command.envelope.scope,
            "payload": command.envelope.payload,
        }))
        .ok()
    }

    fn replay_lookup(&self, command: &ValidatedCommand) -> Option<CommandAcknowledgementV2> {
        let key = command.envelope.idempotency_key.as_ref()?;
        let fingerprint = Self::fingerprint(command)?;
        let replay = self
            .replay
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        replay.get(key).map(|record| {
            if record.fingerprint != fingerprint {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::DuplicateReplayConflict,
                );
            }
            let mut acknowledgement = record.acknowledgement.clone();
            acknowledgement.command_id = command.envelope.command_id.clone();
            acknowledgement.request_id = command.envelope.request_id.clone();
            acknowledgement.correlation_id = command.envelope.correlation_id.clone();
            acknowledgement.timestamp = Utc::now();
            acknowledgement
        })
    }

    fn remember_replay(
        &self,
        command: &ValidatedCommand,
        acknowledgement: &CommandAcknowledgementV2,
    ) {
        let Some(key) = command.envelope.idempotency_key.as_ref() else {
            return;
        };
        let Some(fingerprint) = Self::fingerprint(command) else {
            return;
        };
        self.replay
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(key.clone())
            .or_insert_with(|| ReplayRecord {
                fingerprint,
                acknowledgement: acknowledgement.clone(),
            });
    }

    /// Drain a bounded event batch. One transport consumer should own this call.
    pub fn drain_events(&self, limit: usize, timeout: Duration) -> EventDrain {
        self.events.drain(limit, timeout)
    }

    /// Build an authoritative snapshot for gap recovery or process recreation.
    pub fn snapshot(
        &self,
        domain: RuntimeSnapshotDomain,
    ) -> Result<RuntimeSnapshotEnvelope, RuntimeError> {
        if self.closed.load(Ordering::Acquire) && domain != RuntimeSnapshotDomain::Application {
            return Err(RuntimeError::Stopped);
        }
        let payload = match domain {
            RuntimeSnapshotDomain::Application => json!({
                "state": self.state(),
                "runtime_session_id": self.session_id,
                "app_data_dir": self.config.app_data_dir,
                "cancelled": self.cancellation.is_cancelled(),
            }),
            RuntimeSnapshotDomain::Settings => json!({
                "values": self
                    .settings
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone(),
            }),
            RuntimeSnapshotDomain::Protocol => serde_json::to_value(self.capabilities())
                .map_err(|_| RuntimeError::UnsupportedSnapshot)?,
            RuntimeSnapshotDomain::Operations => json!({
                "replay_entries": self
                    .replay
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .len(),
            }),
        };
        Ok(RuntimeSnapshotEnvelope {
            runtime_session_id: self.session_id.clone(),
            domain,
            schema_version: 1,
            generated_at: Utc::now(),
            payload,
        })
    }

    /// Begin deterministic shutdown. Repeated calls are idempotent.
    pub fn shutdown(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        *self
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = RuntimeState::Stopping;
        self.events.emit(
            "application:lifecycle",
            "runtime.stopping",
            json!({ "runtime_session_id": self.session_id }),
            None,
            None,
        );
        self.cancellation.cancel();
        self.async_runtime.handle().spawn(async {});
        *self
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = RuntimeState::Stopped;
        self.events.emit(
            "application:lifecycle",
            "runtime.stopped",
            json!({ "runtime_session_id": self.session_id }),
            None,
            None,
        );
    }
}

impl Drop for MukeiRuntime {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.cancellation.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_protocol::{CommandScope, ProtocolVersion};

    fn runtime_with_capacity(event_capacity: usize) -> MukeiRuntime {
        MukeiRuntime::create(RuntimeConfig {
            app_data_dir: "/tmp/mukei-test".into(),
            worker_threads: 1,
            max_blocking_threads: 2,
            event_capacity,
        })
        .expect("runtime")
    }

    fn runtime() -> MukeiRuntime {
        runtime_with_capacity(64)
    }

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
            scope: None::<CommandScope>,
            payload: json!({ "config_path": "/tmp/mukei-test/config.toml" }),
        }
    }

    #[test]
    fn initializes_and_emits_events() {
        let runtime = runtime();
        let acknowledgement = runtime.submit(initialize_command());
        assert_eq!(runtime.state(), RuntimeState::Ready);
        assert_eq!(acknowledgement.status, AcknowledgementStatus::Accepted);
        assert!(!runtime
            .drain_events(16, Duration::from_millis(1))
            .events
            .is_empty());
    }

    #[test]
    fn capabilities_only_advertise_implemented_commands() {
        let capabilities = runtime().capabilities().capabilities;
        assert!(capabilities.contains(&"command:app.initialize".to_owned()));
        assert!(capabilities.contains(&"command:settings.update".to_owned()));
        assert!(!capabilities.contains(&"command:chat.send_message".to_owned()));
        assert!(capabilities.contains(&CAP_EVENT_GAP_REPORTING.to_owned()));
    }

    #[test]
    fn event_bus_reports_overflow_and_more_state() {
        let bus = EventBus::new(1);
        bus.emit("test", "test.one", json!({}), None, None);
        bus.emit("test", "test.two", json!({}), None, None);
        let first = bus.drain(1, Duration::ZERO);
        assert_eq!(first.events[0].event_type, "runtime.event_gap");
        assert!(first.has_more);
        let second = bus.drain(1, Duration::ZERO);
        assert_eq!(second.events[0].event_type, "test.one");
        assert!(!second.has_more);
    }

    #[test]
    fn idempotent_replay_rebinds_request_correlation() {
        let runtime = runtime();
        runtime.submit(initialize_command());
        let mut first = initialize_command();
        first.command_id = "cmd-chat-one".into();
        first.request_id = "req-chat-one".into();
        first.correlation_id = "corr-chat-one".into();
        first.command_type = "chat.send_message".into();
        first.idempotency_key = Some("idem-chat".into());
        first.payload = json!({ "text": "hello" });
        let first_ack = runtime.submit(first.clone());
        assert_eq!(
            first_ack.rejection_reason,
            Some(RejectionReason::CapabilityUnavailable)
        );

        let mut replay = first;
        replay.command_id = "cmd-chat-two".into();
        replay.request_id = "req-chat-two".into();
        replay.correlation_id = "corr-chat-two".into();
        let replay_ack = runtime.submit(replay.clone());
        assert_eq!(replay_ack.command_id, replay.command_id);
        assert_eq!(replay_ack.request_id, replay.request_id);
        assert_eq!(replay_ack.correlation_id, replay.correlation_id);
        assert_eq!(
            replay_ack.rejection_reason,
            Some(RejectionReason::CapabilityUnavailable)
        );
    }

    #[test]
    fn shutdown_is_idempotent_and_snapshot_remains_available() {
        let runtime = runtime();
        runtime.shutdown();
        runtime.shutdown();
        assert_eq!(runtime.state(), RuntimeState::Stopped);
        assert!(runtime
            .snapshot(RuntimeSnapshotDomain::Application)
            .is_ok());
    }
}
