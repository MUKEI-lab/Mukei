//! Local protocol-v2 adapter state and envelope helpers.
//!
//! This module owns only bridge-boundary protocol concerns: validation preflight,
//! idempotency replay protection, command correlation metadata, per-stream sequencing,
//! and conversion of existing v1 bridge events into reliable v2 envelopes.

use std::collections::{HashMap, VecDeque};
use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::{json, Value};

use mukei_core::ui_contract::{BridgeEvent, BridgeEventKind, ChatTurnState, DownloadState};
use mukei_core::ui_protocol::{
    validate_command, CommandAcknowledgementV2, CommandEnvelopeV2, CommandScope, CommandType,
    EventEnvelopeV2, ProtocolVersion, RejectionReason, ValidatedCommand, ValidatedCommandPayload,
    MAX_COMMAND_ENVELOPE_BYTES,
};

use crate::bridge_state::RuntimePhase;
use crate::{ffi, runtime_state};

const MAX_IDEMPOTENCY_ENTRIES: usize = 512;
const MAX_ACTIVE_CONTEXTS: usize = 128;

/// Correlation metadata retained while an accepted command produces events.
#[derive(Clone, Debug)]
pub(crate) struct CommandContext {
    pub(crate) command_id: String,
    pub(crate) request_id: String,
    pub(crate) correlation_id: String,
    pub(crate) operation_id: String,
    pub(crate) command_type: String,
    pub(crate) scope: Option<CommandScope>,
}

#[derive(Clone, Debug)]
struct IdempotencyEntry {
    fingerprint: String,
    acknowledgement: CommandAcknowledgementV2,
}

/// Process-owned protocol state stored inside the existing bridge runtime owner.
pub(crate) struct ProtocolRuntimeState {
    sequence_by_stream: HashMap<String, u64>,
    contexts: HashMap<String, CommandContext>,
    idempotency: HashMap<String, IdempotencyEntry>,
    idempotency_order: VecDeque<String>,
}

impl ProtocolRuntimeState {
    pub(crate) fn new() -> Self {
        Self {
            sequence_by_stream: HashMap::new(),
            contexts: HashMap::new(),
            idempotency: HashMap::new(),
            idempotency_order: VecDeque::new(),
        }
    }

    fn next_sequence(&mut self, stream_id: &str) -> u64 {
        let next = self
            .sequence_by_stream
            .entry(stream_id.to_string())
            .or_insert(0);
        *next = next.saturating_add(1);
        *next
    }

    fn fingerprint(command: &ValidatedCommand) -> String {
        serde_json::to_string(&json!({
            "operation_id": &command.envelope.operation_id,
            "command_type": command.command_type.as_str(),
            "scope": &command.envelope.scope,
            "payload": &command.envelope.payload,
        }))
        .unwrap_or_else(|_| command.command_type.as_str().to_string())
    }

    pub(crate) fn replay_acknowledgement(
        &self,
        command: &ValidatedCommand,
    ) -> Result<Option<CommandAcknowledgementV2>, RejectionReason> {
        let Some(key) = command.envelope.idempotency_key.as_deref() else {
            return Ok(None);
        };
        let Some(entry) = self.idempotency.get(key) else {
            return Ok(None);
        };
        if entry.fingerprint == Self::fingerprint(command) {
            let mut acknowledgement = entry.acknowledgement.clone();
            acknowledgement.command_id = command.envelope.command_id.clone();
            acknowledgement.request_id = command.envelope.request_id.clone();
            acknowledgement.correlation_id = command.envelope.correlation_id.clone();
            acknowledgement.timestamp = chrono::Utc::now();
            Ok(Some(acknowledgement))
        } else {
            Err(RejectionReason::DuplicateReplayConflict)
        }
    }

    pub(crate) fn remember_idempotency(
        &mut self,
        command: &ValidatedCommand,
        acknowledgement: &CommandAcknowledgementV2,
    ) {
        let Some(key) = command.envelope.idempotency_key.clone() else {
            return;
        };
        if !self.idempotency.contains_key(&key) {
            self.idempotency_order.push_back(key.clone());
        }
        self.idempotency.insert(
            key,
            IdempotencyEntry {
                fingerprint: Self::fingerprint(command),
                acknowledgement: acknowledgement.clone(),
            },
        );
        while self.idempotency_order.len() > MAX_IDEMPOTENCY_ENTRIES {
            if let Some(expired) = self.idempotency_order.pop_front() {
                self.idempotency.remove(&expired);
            }
        }
    }

    pub(crate) fn rollback_accepted_command(
        &mut self,
        command: &ValidatedCommand,
        operation_id: &str,
    ) {
        self.contexts
            .retain(|_, context| context.operation_id != operation_id);
        if let Some(key) = command.envelope.idempotency_key.as_deref() {
            let remove = self
                .idempotency
                .get(key)
                .and_then(|entry| entry.acknowledgement.operation_id.as_deref())
                == Some(operation_id);
            if remove {
                self.idempotency.remove(key);
                self.idempotency_order.retain(|value| value != key);
            }
        }
    }

    pub(crate) fn has_pending_acceptance_conflict(&self, command: &ValidatedCommand) -> bool {
        match command.command_type {
            CommandType::AppInitialize => self.contexts.contains_key("application:lifecycle"),
            CommandType::ChatSendMessage
            | CommandType::RecoveryResume
            | CommandType::RecoveryRegenerate => self.contexts.contains_key("chat:active"),
            CommandType::ModelDownload => match &command.payload {
                ValidatedCommandPayload::ModelDownload(value) => self
                    .contexts
                    .contains_key(&format!("download:model:{}", value.model_id)),
                _ => false,
            },
            _ => false,
        }
    }

    pub(crate) fn validate_chat_cancellation_target(
        &self,
        operation_id: &str,
        scope: Option<&CommandScope>,
    ) -> Result<(), RejectionReason> {
        let Some(active) = self.contexts.get("chat:active") else {
            return Err(RejectionReason::CapabilityUnavailable);
        };
        if active.operation_id != operation_id {
            return Err(RejectionReason::StaleScope);
        }
        let Some(requested_scope) = scope else {
            return Err(RejectionReason::StaleScope);
        };
        if let Some(active_scope) = active.scope.as_ref() {
            if active_scope.conversation_id.as_deref() != requested_scope.conversation_id.as_deref()
                || active_scope.branch_id.as_deref() != requested_scope.branch_id.as_deref()
            {
                return Err(RejectionReason::StaleScope);
            }
            if let (Some(active_turn), Some(requested_turn)) = (
                active_scope.turn_id.as_deref(),
                requested_scope.turn_id.as_deref(),
            ) {
                if active_turn != requested_turn {
                    return Err(RejectionReason::StaleScope);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn register_context(&mut self, command: &ValidatedCommand, context: CommandContext) {
        let key = context_key_for_command(command, &context.operation_id);
        self.contexts.insert(key, context);
        if self.contexts.len() > MAX_ACTIVE_CONTEXTS {
            // Active command count is normally tiny. If a faulty producer leaks contexts,
            // drop only generic completed-command candidates before risking unbounded memory.
            let stale_generic = self
                .contexts
                .keys()
                .find(|key| key.starts_with("operation:"))
                .cloned();
            if let Some(key) = stale_generic {
                self.contexts.remove(&key);
            }
        }
    }

    fn context_for_event(
        &mut self,
        stream_id: &str,
        event: &BridgeEvent,
    ) -> Option<CommandContext> {
        if let Some(context) = self.contexts.get(stream_id).cloned() {
            return Some(context);
        }
        let fallback = match &event.kind {
            BridgeEventKind::ChatState { .. }
            | BridgeEventKind::ChatChunk { .. }
            | BridgeEventKind::ChatCompleted
            | BridgeEventKind::ChatCancelled
            | BridgeEventKind::ChatFailed { .. } => self.contexts.get("chat:active").cloned(),
            BridgeEventKind::DownloadState { model_id, .. }
            | BridgeEventKind::DownloadProgress { model_id, .. }
            | BridgeEventKind::DownloadCompleted { model_id, .. } => model_id
                .as_deref()
                .and_then(|id| self.contexts.get(&format!("download:model:{id}")).cloned())
                .or_else(|| self.contexts.get("download:active").cloned()),
            BridgeEventKind::DownloadFailed { .. } => self.single_download_context(),
            BridgeEventKind::AppLifecycle { .. } => {
                self.contexts.get("application:lifecycle").cloned()
            }
            BridgeEventKind::Error { error } => match error.source.as_str() {
                "initialize" => self.contexts.get("application:lifecycle").cloned(),
                "send_message" | "recover_interrupted_turn" => {
                    self.contexts.get("chat:active").cloned()
                }
                "download_model" => self.single_download_context(),
                _ => None,
            },
            BridgeEventKind::CapabilitySnapshot { .. } => None,
        };
        if let Some(mut context) = fallback.clone() {
            if matches!(
                &event.kind,
                BridgeEventKind::ChatState { .. }
                    | BridgeEventKind::ChatChunk { .. }
                    | BridgeEventKind::ChatCompleted
                    | BridgeEventKind::ChatCancelled
                    | BridgeEventKind::ChatFailed { .. }
            ) {
                if let (Some(conversation_id), Some(branch_id)) =
                    (event.conversation_id, event.branch_id)
                {
                    context.scope = Some(CommandScope {
                        conversation_id: Some(conversation_id.0.to_string()),
                        branch_id: Some(branch_id.0.to_string()),
                        turn_id: event.turn_id.clone(),
                        model_id: None,
                        document_id: None,
                    });
                    self.contexts
                        .insert("chat:active".to_string(), context.clone());
                }
            }
            if stream_id != "application:lifecycle" && !stream_id.starts_with("operation:") {
                self.contexts.insert(stream_id.to_string(), context);
            }
        }
        fallback
    }

    fn single_download_context(&self) -> Option<CommandContext> {
        let mut found = None;
        for (key, value) in &self.contexts {
            if key.starts_with("download:model:") || key == "download:active" {
                if found.is_some() {
                    return None;
                }
                found = Some(value.clone());
            }
        }
        found
    }

    fn retire_terminal_context(
        &mut self,
        stream_id: &str,
        event: &BridgeEvent,
        context: Option<&CommandContext>,
    ) {
        let terminal = match &event.kind {
            BridgeEventKind::ChatCompleted | BridgeEventKind::ChatCancelled => true,
            BridgeEventKind::ChatState { state, .. } => matches!(
                state,
                ChatTurnState::Failed | ChatTurnState::Completed | ChatTurnState::Cancelled
            ),
            BridgeEventKind::DownloadState { state, .. } => matches!(
                state,
                DownloadState::Completed | DownloadState::Failed | DownloadState::Cancelled
            ),
            BridgeEventKind::AppLifecycle { state, .. } => matches!(
                state,
                mukei_core::ui_contract::AppLifecycleState::Ready
                    | mukei_core::ui_contract::AppLifecycleState::Degraded
                    | mukei_core::ui_contract::AppLifecycleState::FatalError
            ),
            BridgeEventKind::Error { .. } => context.is_some(),
            _ => false,
        };
        if !terminal {
            return;
        }
        self.contexts.remove(stream_id);
        if let Some(context) = context {
            self.contexts
                .retain(|_, value| value.operation_id != context.operation_id);
        }
    }

    pub(crate) fn wrap_bridge_event(&mut self, event: BridgeEvent) -> String {
        let stream_id = stream_id_for_bridge_event(&event);
        let context = self.context_for_event(&stream_id, &event);
        let sequence = self.next_sequence(&stream_id);
        let mut payload = serde_json::to_value(&event).unwrap_or_else(|_| {
            json!({
                "schema_version": BridgeEvent::SCHEMA_VERSION,
                "timestamp": chrono::Utc::now(),
                "category": "error",
                "error": {
                    "code": "ERR_EVENT_SERIALIZE",
                    "class": "bridge",
                    "severity": "error",
                    "recoverable": true,
                    "user_message": "Bridge event could not be serialized.",
                    "technical_message": "Bridge event serialization failed.",
                    "suggested_action": "retry",
                    "source": "bridge"
                }
            })
        });
        let event_type = payload
            .get("category")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        if let Value::Object(ref mut map) = payload {
            // The v1 sequence was bridge-global and cannot be compared safely across independent
            // streams. Preserve it only as explicit legacy metadata.
            if let Some(old_sequence) = map.remove("sequence") {
                map.insert("legacy_sequence".into(), old_sequence);
            }
        }
        let envelope = EventEnvelopeV2 {
            protocol_version: ProtocolVersion::CURRENT,
            event_id: uuid::Uuid::new_v4().to_string(),
            stream_id: stream_id.clone(),
            sequence,
            event_type,
            emitted_at: chrono::Utc::now(),
            correlation_id: context.as_ref().map(|value| value.correlation_id.clone()),
            operation_id: context.as_ref().map(|value| value.operation_id.clone()),
            request_id: context.as_ref().map(|value| value.request_id.clone()),
            command_id: context.as_ref().map(|value| value.command_id.clone()),
            command_type: context.as_ref().map(|value| value.command_type.clone()),
            payload,
        };
        self.retire_terminal_context(&stream_id, &event, context.as_ref());
        serde_json::to_string(&envelope)
            .unwrap_or_else(|_| fallback_v2_error_json(&stream_id, sequence))
    }

    pub(crate) fn operation_event_json(
        &mut self,
        context: &CommandContext,
        state: &str,
        result: Option<Value>,
        error: Option<Value>,
    ) -> String {
        let stream_id = format!("operation:{}", context.operation_id);
        let sequence = self.next_sequence(&stream_id);
        let envelope = EventEnvelopeV2 {
            protocol_version: ProtocolVersion::CURRENT,
            event_id: uuid::Uuid::new_v4().to_string(),
            stream_id: stream_id.clone(),
            sequence,
            event_type: "operation_lifecycle".into(),
            emitted_at: chrono::Utc::now(),
            correlation_id: Some(context.correlation_id.clone()),
            operation_id: Some(context.operation_id.clone()),
            request_id: Some(context.request_id.clone()),
            command_id: Some(context.command_id.clone()),
            command_type: Some(context.command_type.clone()),
            payload: json!({
                "category": "operation_lifecycle",
                "state": state,
                "result": result,
                "error": error,
            }),
        };
        if matches!(state, "completed" | "failed" | "cancelled") {
            self.contexts
                .retain(|_, value| value.operation_id != context.operation_id);
        }
        serde_json::to_string(&envelope)
            .unwrap_or_else(|_| fallback_v2_error_json(&stream_id, sequence))
    }
}

fn fallback_v2_error_json(stream_id: &str, sequence: u64) -> String {
    json!({
        "protocol_version": {"major": 2, "minor": 0},
        "event_id": uuid::Uuid::new_v4().to_string(),
        "stream_id": stream_id,
        "sequence": sequence,
        "event_type": "error",
        "emitted_at": chrono::Utc::now(),
        "payload": {
            "category": "error",
            "error": {
                "code": "ERR_EVENT_SERIALIZE",
                "class": "bridge",
                "severity": "error",
                "recoverable": true,
                "user_message": "Bridge event could not be serialized.",
                "technical_message": "Protocol-v2 event serialization failed.",
                "suggested_action": "retry",
                "source": "bridge"
            }
        }
    })
    .to_string()
}

fn stream_id_for_bridge_event(event: &BridgeEvent) -> String {
    match &event.kind {
        BridgeEventKind::AppLifecycle { .. } | BridgeEventKind::CapabilitySnapshot { .. } => {
            "application:lifecycle".into()
        }
        BridgeEventKind::ChatState { .. }
        | BridgeEventKind::ChatChunk { .. }
        | BridgeEventKind::ChatCompleted
        | BridgeEventKind::ChatCancelled
        | BridgeEventKind::ChatFailed { .. } => match (event.conversation_id, event.branch_id) {
            (Some(conversation), Some(branch)) => {
                format!("conversation:{}:branch:{}", conversation.0, branch.0)
            }
            _ => "chat:active".into(),
        },
        BridgeEventKind::DownloadState {
            model_id,
            destination,
            ..
        }
        | BridgeEventKind::DownloadProgress {
            model_id,
            destination,
            ..
        } => model_id
            .as_deref()
            .map(|id| format!("download:model:{id}"))
            .or_else(|| destination.as_deref().map(|id| format!("download:{id}")))
            .unwrap_or_else(|| "download:active".into()),
        BridgeEventKind::DownloadCompleted { model_id, .. } => model_id
            .as_deref()
            .map(|id| format!("download:model:{id}"))
            .unwrap_or_else(|| "download:active".into()),
        BridgeEventKind::DownloadFailed { .. } => "download:active".into(),
        BridgeEventKind::Error { error } => match error.source.as_str() {
            "initialize" => "application:lifecycle".into(),
            "send_message" | "recover_interrupted_turn" => "chat:active".into(),
            "download_model" => "download:active".into(),
            _ => "application:errors".into(),
        },
    }
}

fn context_key_for_command(command: &ValidatedCommand, operation_id: &str) -> String {
    match command.command_type {
        CommandType::AppInitialize => "application:lifecycle".into(),
        CommandType::ChatSendMessage
        | CommandType::RecoveryResume
        | CommandType::RecoveryRegenerate => "chat:active".into(),
        CommandType::ModelDownload => match &command.payload {
            ValidatedCommandPayload::ModelDownload(value) => {
                format!("download:model:{}", value.model_id)
            }
            _ => "download:active".into(),
        },
        _ => format!("operation:{operation_id}"),
    }
}

/// Parse, structurally validate, policy-preflight, replay-check, and dispatch one command.
pub(crate) fn submit_command_json(
    agent: Pin<&mut ffi::MukeiAgent>,
    command_json: QString,
) -> QString {
    let raw = command_json.to_string();
    if raw.len() > MAX_COMMAND_ENVELOPE_BYTES {
        return acknowledgement_json(CommandAcknowledgementV2::rejected(
            None,
            RejectionReason::InvalidPayload,
        ));
    }
    let envelope = match serde_json::from_str::<CommandEnvelopeV2>(&raw) {
        Ok(value) => value,
        Err(_) => {
            return acknowledgement_json(CommandAcknowledgementV2::rejected(
                None,
                RejectionReason::InvalidPayload,
            ));
        }
    };
    let command = match validate_command(envelope.clone()) {
        Ok(value) => value,
        Err(reason) => {
            return acknowledgement_json(CommandAcknowledgementV2::rejected(
                Some(&envelope),
                reason,
            ));
        }
    };

    // Resolve idempotent resubmission before new-command busy/availability preflight. A valid
    // replay must return the original accepted operation identity even while that operation is
    // still active; only a key reused for different stable command content is rejected.
    {
        let state = runtime_state().protocol_state().lock();
        match state.replay_acknowledgement(&command) {
            Ok(Some(acknowledgement)) => return acknowledgement_json(acknowledgement),
            Ok(None) => {}
            Err(reason) => {
                return acknowledgement_json(CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    reason,
                ));
            }
        }
    }

    if let Some(reason) = preflight(&agent, &command) {
        return acknowledgement_json(CommandAcknowledgementV2::rejected(
            Some(&command.envelope),
            reason,
        ));
    }

    let operation_id = command
        .envelope
        .operation_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let acknowledgement = CommandAcknowledgementV2::accepted(
        &command.envelope,
        command
            .command_type
            .creates_operation()
            .then_some(operation_id.clone()),
    );
    let context = CommandContext {
        command_id: command.envelope.command_id.clone(),
        request_id: command.envelope.request_id.clone(),
        correlation_id: command.envelope.correlation_id.clone(),
        operation_id,
        command_type: command.command_type.as_str().into(),
        scope: command.envelope.scope.clone(),
    };
    {
        let mut state = runtime_state().protocol_state().lock();
        state.register_context(&command, context.clone());
        state.remember_idempotency(&command, &acknowledgement);
    }

    // Return the acknowledgement from the acceptance boundary before execution can emit a
    // completion event. Dispatch remains on the existing QObjects/runtime owner and adapts into
    // the existing backend methods; no second runtime or domain implementation is introduced.
    let qt = agent.as_ref().get_ref().qt_thread();
    let dispatch_command = command.clone();
    let dispatch_context = context.clone();
    if qt
        .queue(move |mut qobject| {
            dispatch_validated_command(qobject.as_mut(), dispatch_command, dispatch_context);
        })
        .is_err()
    {
        runtime_state()
            .protocol_state()
            .lock()
            .rollback_accepted_command(&command, &context.operation_id);
        return acknowledgement_json(CommandAcknowledgementV2::rejected(
            Some(&command.envelope),
            RejectionReason::BackendUnavailable,
        ));
    }
    acknowledgement_json(acknowledgement)
}

fn acknowledgement_json(acknowledgement: CommandAcknowledgementV2) -> QString {
    QString::from(
        serde_json::to_string(&acknowledgement)
            .unwrap_or_else(|_| {
                json!({
                    "protocol_version": {"major": 2, "minor": 0},
                    "command_id": acknowledgement.command_id,
                    "request_id": acknowledgement.request_id,
                    "correlation_id": acknowledgement.correlation_id,
                    "status": "rejected",
                    "rejection_reason": "backend_unavailable",
                    "timestamp": chrono::Utc::now(),
                })
                .to_string()
            })
            .as_str(),
    )
}

fn preflight(
    agent: &Pin<&mut ffi::MukeiAgent>,
    command: &ValidatedCommand,
) -> Option<RejectionReason> {
    if runtime_state()
        .protocol_state()
        .lock()
        .has_pending_acceptance_conflict(command)
    {
        return Some(RejectionReason::BusyConflict);
    }
    let ready = runtime_state().runtime_coordinator().is_ready();
    match command.command_type {
        CommandType::AppInitialize => {
            if ready
                || !matches!(
                    runtime_state().runtime_coordinator().phase(),
                    RuntimePhase::Uninitialized | RuntimePhase::Quarantined
                )
            {
                return Some(RejectionReason::BusyConflict);
            }
        }
        _ if !ready => return Some(RejectionReason::BackendUnavailable),
        _ => {}
    }

    match command.command_type {
        CommandType::ChatSendMessage => {
            if agent
                .as_ref()
                .rust()
                .busy
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return Some(RejectionReason::BusyConflict);
            }
        }
        CommandType::RecoveryResume | CommandType::RecoveryRegenerate => {
            if agent
                .as_ref()
                .rust()
                .busy
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return Some(RejectionReason::BusyConflict);
            }
            if !cfg!(feature = "rusqlite") {
                return Some(RejectionReason::CapabilityUnavailable);
            }
            #[cfg(feature = "rusqlite")]
            {
                let Some(pool) = runtime_state().database_pool() else {
                    return Some(RejectionReason::BackendUnavailable);
                };
                let Some(scope) = command.envelope.scope.as_ref() else {
                    return Some(RejectionReason::StaleScope);
                };
                let current = mukei_core::runtime::get().block_on(async move {
                    mukei_core::storage::RecoveryStore::interrupted_turn(&pool).await
                });
                match current {
                    Ok(Some(turn)) => {
                        let conversation_id = turn.conversation.0.to_string();
                        let branch_id = turn.branch.0.to_string();
                        if scope.conversation_id.as_deref() != Some(conversation_id.as_str())
                            || scope.branch_id.as_deref() != Some(branch_id.as_str())
                        {
                            return Some(RejectionReason::StaleScope);
                        }
                    }
                    Ok(None) => return Some(RejectionReason::StaleScope),
                    Err(_) => return Some(RejectionReason::BackendUnavailable),
                }
            }
        }
        CommandType::ChatStopGeneration => {
            if !agent
                .as_ref()
                .rust()
                .busy
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return Some(RejectionReason::CapabilityUnavailable);
            }
            let Some(operation_id) = command.envelope.operation_id.as_deref() else {
                return Some(RejectionReason::StaleScope);
            };
            if let Err(reason) = runtime_state()
                .protocol_state()
                .lock()
                .validate_chat_cancellation_target(operation_id, command.envelope.scope.as_ref())
            {
                return Some(reason);
            }
        }
        CommandType::ModelDownload => {
            if !cfg!(feature = "network") {
                return Some(RejectionReason::CapabilityUnavailable);
            }
            if let ValidatedCommandPayload::ModelDownload(value) = &command.payload {
                if agent
                    .as_ref()
                    .rust()
                    .active_downloads
                    .lock()
                    .iter()
                    .any(|download| download.model_id.as_deref() == Some(value.model_id.as_str()))
                {
                    return Some(RejectionReason::BusyConflict);
                }
            }
        }
        CommandType::DownloadCancel => {
            if agent.as_ref().rust().active_downloads.lock().is_empty() {
                return Some(RejectionReason::CapabilityUnavailable);
            }
        }
        CommandType::ModelSelect | CommandType::ModelDelete => {
            if agent
                .as_ref()
                .rust()
                .busy
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return Some(RejectionReason::BusyConflict);
            }
            if let ValidatedCommandPayload::Model(value) = &command.payload {
                if mukei_core::engine::lookup_model_str(&value.model_id).is_none() {
                    return Some(RejectionReason::InvalidPayload);
                }
                if command.command_type == CommandType::ModelDelete
                    && agent
                        .as_ref()
                        .rust()
                        .active_downloads
                        .lock()
                        .iter()
                        .any(|download| {
                            download.model_id.as_deref() == Some(value.model_id.as_str())
                        })
                {
                    return Some(RejectionReason::BusyConflict);
                }
            }
        }
        CommandType::SettingsUpdate => {
            if let ValidatedCommandPayload::SettingUpdate(value) = &command.payload {
                let lowered = value.key.to_ascii_lowercase();
                if lowered.contains("secret")
                    || lowered.contains("token")
                    || lowered.contains("api_key")
                {
                    return Some(RejectionReason::PolicyDenied);
                }
            }
        }
        _ => {}
    }
    None
}

fn dispatch_validated_command(
    mut agent: Pin<&mut ffi::MukeiAgent>,
    command: ValidatedCommand,
    context: CommandContext,
) {
    match (command.command_type, command.payload) {
        (CommandType::AppInitialize, ValidatedCommandPayload::Initialize(value)) => {
            agent.as_mut().initialize(QString::from(&value.config_path));
        }
        (CommandType::ChatSendMessage, ValidatedCommandPayload::SendMessage(value)) => {
            agent.as_mut().send_message(QString::from(&value.text));
        }
        (CommandType::ChatStopGeneration, ValidatedCommandPayload::Empty) => {
            agent.as_mut().stop_generation();
            let event = QString::from(
                runtime_state()
                    .protocol_state()
                    .lock()
                    .operation_event_json(
                        &context,
                        "cancelling",
                        Some(json!({"cancel_requested": true})),
                        None,
                    )
                    .as_str(),
            );
            let qt = agent.as_ref().get_ref().qt_thread();
            let _ = qt.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event);
            });
        }
        (CommandType::ChatClearConversation, ValidatedCommandPayload::Empty) => {
            agent.as_mut().clear_conversation();
            emit_immediate_operation(agent.as_mut(), &context, true, json!({"cleared": true}));
        }
        (CommandType::ModelDownload, ValidatedCommandPayload::ModelDownload(value)) => {
            agent
                .as_mut()
                .download_model(QString::from(&value.model_id), QString::from(&value.sha256));
        }
        (CommandType::DownloadCancel, ValidatedCommandPayload::Empty) => {
            agent.as_mut().stop_download();
            emit_immediate_operation(
                agent.as_mut(),
                &context,
                true,
                json!({"cancel_requested": true}),
            );
        }
        (CommandType::ModelSelect, ValidatedCommandPayload::Model(value)) => {
            let result = agent
                .as_mut()
                .select_installed_model_json(QString::from(&value.model_id));
            emit_json_result_operation(agent.as_mut(), &context, result.to_string());
        }
        (CommandType::ModelDelete, ValidatedCommandPayload::Model(value)) => {
            let result = agent
                .as_mut()
                .delete_installed_model_json(QString::from(&value.model_id));
            emit_json_result_operation(agent.as_mut(), &context, result.to_string());
        }
        (CommandType::DocumentGrant, ValidatedCommandPayload::DocumentGrant(value)) => {
            let result = agent.as_mut().grant_document_access_json(
                QString::from(&value.target),
                QString::from(&value.label),
                QString::from(&value.mime_type),
            );
            emit_json_result_operation(agent.as_mut(), &context, result.to_string());
        }
        (CommandType::DocumentRevoke, ValidatedCommandPayload::Document(value)) => {
            let result = agent
                .as_mut()
                .revoke_document_json(QString::from(&value.document_id));
            emit_json_result_operation(agent.as_mut(), &context, result.to_string());
        }
        (CommandType::DocumentRetryIngestion, ValidatedCommandPayload::Document(value)) => {
            let result = agent
                .as_mut()
                .retry_document_ingestion_json(QString::from(&value.document_id));
            emit_json_result_operation(agent.as_mut(), &context, result.to_string());
        }
        (CommandType::SettingsUpdate, ValidatedCommandPayload::SettingUpdate(value)) => {
            crate::dispatch_protocol_setting_update(
                agent.as_mut(),
                context,
                value.key,
                value.value,
            );
        }
        (CommandType::RecoveryResume, ValidatedCommandPayload::Empty) => {
            agent.as_mut().resume_interrupted_turn();
        }
        (CommandType::RecoveryRegenerate, ValidatedCommandPayload::Empty) => {
            agent.as_mut().regenerate_interrupted_turn();
        }
        _ => {
            emit_immediate_operation(
                agent.as_mut(),
                &context,
                false,
                json!({"code": "ERR_PROTOCOL_DISPATCH_MISMATCH"}),
            );
        }
    }
}

fn emit_json_result_operation(
    mut agent: Pin<&mut ffi::MukeiAgent>,
    context: &CommandContext,
    raw: String,
) {
    let parsed = serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| {
        json!({
            "ok": false,
            "error": {"code": "ERR_PROTOCOL_INVALID_RESULT", "safe_message": "Backend returned an invalid operation result."}
        })
    });
    let ok = parsed.get("ok").and_then(Value::as_bool) == Some(true);
    emit_immediate_operation(agent.as_mut(), context, ok, parsed);
}

fn emit_immediate_operation(
    agent: Pin<&mut ffi::MukeiAgent>,
    context: &CommandContext,
    ok: bool,
    payload: Value,
) {
    let (result, error) = if ok {
        (Some(payload), None)
    } else {
        let error = payload
            .get("error")
            .cloned()
            .unwrap_or_else(|| payload.clone());
        (None, Some(error))
    };
    let event = QString::from(
        runtime_state()
            .protocol_state()
            .lock()
            .operation_event_json(
                context,
                if ok { "completed" } else { "failed" },
                result,
                error,
            )
            .as_str(),
    );
    let qt = agent.as_ref().get_ref().qt_thread();
    let _ = qt.queue(move |mut qobject| {
        qobject.as_mut().event_emitted(event);
    });
}

/// Build one terminal operation event JSON from an async protocol command.
pub(crate) fn async_operation_event_json(
    context: &CommandContext,
    ok: bool,
    result: Option<Value>,
    error: Option<Value>,
) -> QString {
    let json = runtime_state()
        .protocol_state()
        .lock()
        .operation_event_json(
            context,
            if ok { "completed" } else { "failed" },
            result,
            error,
        );
    QString::from(json.as_str())
}

#[cfg(test)]
mod sol02_tests {
    use super::*;
    use chrono::Utc;

    fn send_envelope(
        command_id: &str,
        request_id: &str,
        correlation_id: &str,
        key: &str,
        text: &str,
    ) -> CommandEnvelopeV2 {
        CommandEnvelopeV2 {
            protocol_version: ProtocolVersion::CURRENT,
            command_id: command_id.into(),
            request_id: request_id.into(),
            command_type: "chat.send_message".into(),
            submitted_at: Utc::now(),
            operation_id: None,
            correlation_id: correlation_id.into(),
            idempotency_key: Some(key.into()),
            scope: Some(CommandScope {
                conversation_id: Some("conversation-a".into()),
                branch_id: Some("branch-a".into()),
                turn_id: Some("turn-a".into()),
                model_id: None,
                document_id: None,
            }),
            payload: serde_json::json!({"text": text}),
        }
    }

    #[test]
    fn sol02_idempotent_replay_rebinds_transport_ids() {
        let first = validate_command(send_envelope(
            "command-a",
            "request-a",
            "correlation-a",
            "idem-a",
            "hello",
        ))
        .unwrap();
        let acknowledgement =
            CommandAcknowledgementV2::accepted(&first.envelope, Some("operation-a".into()));
        let mut state = ProtocolRuntimeState::new();
        state.remember_idempotency(&first, &acknowledgement);

        let retry = validate_command(send_envelope(
            "command-b",
            "request-b",
            "correlation-b",
            "idem-a",
            "hello",
        ))
        .unwrap();
        let replay = state.replay_acknowledgement(&retry).unwrap().unwrap();
        assert_eq!(replay.operation_id.as_deref(), Some("operation-a"));
        assert_eq!(replay.command_id, "command-b");
        assert_eq!(replay.request_id, "request-b");
        assert_eq!(replay.correlation_id, "correlation-b");
    }

    #[test]
    fn sol02_conflicting_replay_key_is_rejected() {
        let first = validate_command(send_envelope(
            "command-a",
            "request-a",
            "correlation-a",
            "idem-a",
            "hello",
        ))
        .unwrap();
        let acknowledgement =
            CommandAcknowledgementV2::accepted(&first.envelope, Some("operation-a".into()));
        let mut state = ProtocolRuntimeState::new();
        state.remember_idempotency(&first, &acknowledgement);

        let conflicting = validate_command(send_envelope(
            "command-b",
            "request-b",
            "correlation-b",
            "idem-a",
            "different",
        ))
        .unwrap();
        assert_eq!(
            state.replay_acknowledgement(&conflicting),
            Err(RejectionReason::DuplicateReplayConflict)
        );
    }

    #[test]
    fn sol02_scoped_cancel_cannot_target_another_operation() {
        let send = validate_command(send_envelope(
            "command-a",
            "request-a",
            "correlation-a",
            "idem-a",
            "hello",
        ))
        .unwrap();
        let mut state = ProtocolRuntimeState::new();
        state.register_context(
            &send,
            CommandContext {
                command_id: send.envelope.command_id.clone(),
                request_id: send.envelope.request_id.clone(),
                correlation_id: send.envelope.correlation_id.clone(),
                operation_id: "operation-a".into(),
                command_type: send.command_type.as_str().into(),
                scope: send.envelope.scope.clone(),
            },
        );

        assert_eq!(
            state.validate_chat_cancellation_target("operation-b", send.envelope.scope.as_ref(),),
            Err(RejectionReason::StaleScope)
        );
        assert!(state
            .validate_chat_cancellation_target("operation-a", send.envelope.scope.as_ref(),)
            .is_ok());
    }
}


#[cfg(test)]
mod canonical_wire_tests {
    use super::*;
    use mukei_core::types::{BranchId, ConversationId};
    use mukei_core::ui_contract::{CapabilitySnapshot, ChatTurnState};

    #[test]
    fn canonical_chat_event_serialization_keeps_scope_in_payload() {
        let conversation = ConversationId::new();
        let branch = BranchId::new();
        let event = BridgeEvent::new(BridgeEventKind::ChatState {
            state: ChatTurnState::Submitting,
            capabilities: CapabilitySnapshot::inferencing(),
        })
        .with_chat_scope(conversation, branch, "turn-wire-contract".to_string());

        let serialized = ProtocolRuntimeState::new().wrap_bridge_event(event);
        let value: serde_json::Value = serde_json::from_str(&serialized).expect("valid event json");
        let stream_id = value["stream_id"].as_str().expect("stream id");

        assert_eq!(value["protocol_version"]["major"], 2);
        assert_eq!(value["event_type"], "chat_state");
        assert_eq!(value["payload"]["conversation_id"], conversation.0.to_string());
        assert_eq!(value["payload"]["branch_id"], branch.0.to_string());
        assert_eq!(value["payload"]["turn_id"], "turn-wire-contract");
        assert!(value.get("conversation_id").is_none());
        assert!(value.get("branch_id").is_none());
        assert!(stream_id.contains(&conversation.0.to_string()));
        assert!(stream_id.contains(&branch.0.to_string()));
    }
}
