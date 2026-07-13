from pathlib import Path


def replace_once(path: str, old: str, new: str, label: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one anchor, found {count}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")
    print(f"PASS {label}")


# ---------------------------------------------------------------------------
# Core activation: expose an authoritative active-model snapshot independent
# from candidate verification/activation state.
# ---------------------------------------------------------------------------
replace_once(
    "rust/crates/mukei-core/src/engine/activation.rs",
    '''pub struct InferenceReadinessSnapshot {
    pub inference_interface_exists: bool,
    pub real_backend_implementation_available: bool,
    pub selected_model_exists: bool,
    pub selected_model_verified: bool,
    pub activation_in_progress: bool,
    pub active_backend_ready: bool,
    pub development_mock_active: bool,
    pub activation_failed: bool,
    pub product_ready: bool,
    pub state: ModelActivationState,
}

#[async_trait::async_trait]''',
    '''pub struct InferenceReadinessSnapshot {
    pub inference_interface_exists: bool,
    pub real_backend_implementation_available: bool,
    pub selected_model_exists: bool,
    pub selected_model_verified: bool,
    pub activation_in_progress: bool,
    pub active_backend_ready: bool,
    pub development_mock_active: bool,
    pub activation_failed: bool,
    pub product_ready: bool,
    pub state: ModelActivationState,
}

/// Authoritative identity of the backend currently serving inference turns.
/// Candidate verification state is intentionally excluded so a failed or
/// in-progress replacement cannot masquerade as the active model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveModelSnapshot {
    pub model_id: String,
    pub revision: String,
    pub artifact_id: String,
    pub backend: BackendIdentity,
}

#[async_trait::async_trait]''',
    "active model snapshot type",
)
replace_once(
    "rust/crates/mukei-core/src/engine/activation.rs",
    '''    pub fn state(&self) -> ModelActivationState {
        self.inner.read().state.clone()
    }

    pub fn mark_model_missing(''',
    '''    pub fn state(&self) -> ModelActivationState {
        self.inner.read().state.clone()
    }

    pub fn active_model_snapshot(&self) -> Option<ActiveModelSnapshot> {
        self.inner.read().active.as_ref().map(|active| ActiveModelSnapshot {
            model_id: active.model_id.clone(),
            revision: active.revision.clone(),
            artifact_id: active.artifact_id.clone(),
            backend: active.backend.identity(),
        })
    }

    pub fn mark_model_missing(''',
    "active model snapshot accessor",
)
replace_once(
    "rust/crates/mukei-core/src/engine/mod.rs",
    '''pub use activation::{
    ActivationCommit, ActivationFailureCategory, InferenceBackendFactory,
    InferenceReadinessSnapshot, ModelActivationService, ModelActivationState,
    VerifiedModelArtifact, VerifiedModelDescriptor,
};''',
    '''pub use activation::{
    ActivationCommit, ActivationFailureCategory, ActiveModelSnapshot, InferenceBackendFactory,
    InferenceReadinessSnapshot, ModelActivationService, ModelActivationState,
    VerifiedModelArtifact, VerifiedModelDescriptor,
};''',
    "engine active-model export",
)

# ---------------------------------------------------------------------------
# Capability truth: app-ready != model-ready.
# ---------------------------------------------------------------------------
replace_once(
    "rust/crates/mukei-core/src/ui_contract.rs",
    '''    /// Ready capability set for the current bridge implementation.
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
    }''',
    '''    /// Ready capability set with no active inference backend.
    pub fn ready() -> Self {
        Self::ready_with_model(false)
    }

    /// Ready capability set projected from authoritative model readiness.
    pub fn ready_with_model(active_model_ready: bool) -> Self {
        Self {
            can_initialize: false,
            can_send_message: active_model_ready,
            can_stop_generation: false,
            can_download_model: Self::network_enabled(),
            can_stop_download: false,
            can_switch_model: true,
            can_delete_model: true,
            can_clear_conversation: true,
            can_open_settings: true,
            needs_config: false,
            needs_storage_permission: false,
            active_model_ready,
            is_busy: false,
            is_downloading: false,
            is_inferencing: false,
        }
    }''',
    "capability ready-with-model projection",
)
replace_once(
    "rust/crates/mukei-core/src/ui_contract.rs",
    '''            needs_config: false,
            needs_storage_permission: false,
            active_model_ready: false,
            is_busy: true,
            is_downloading: false,
            is_inferencing: true,''',
    '''            needs_config: false,
            needs_storage_permission: false,
            active_model_ready: true,
            is_busy: true,
            is_downloading: false,
            is_inferencing: true,''',
    "inferencing capability requires active model",
)

# ---------------------------------------------------------------------------
# Process-wide runtime owns activation service + optional real factory.
# ---------------------------------------------------------------------------
replace_once(
    "rust/crates/mukei-bridge/src/app_runtime.rs",
    '''use mukei_core::agent::AgentLoop;
use mukei_core::config::MukeiConfig;
use mukei_core::tools::{RemoteFeaturePolicy, ToolRegistry};''',
    '''use mukei_core::agent::AgentLoop;
use mukei_core::config::MukeiConfig;
use mukei_core::engine::{InferenceBackendFactory, ModelActivationService};
use mukei_core::tools::{RemoteFeaturePolicy, ToolRegistry};''',
    "runtime activation imports",
)
replace_once(
    "rust/crates/mukei-bridge/src/app_runtime.rs",
    '''struct AgentServices {
    agent_loop: ParkingMutex<Option<Arc<AgentLoop>>>,
    chat_session: ParkingMutex<Option<(ConversationId, BranchId)>>,
    config: ParkingMutex<Option<MukeiConfig>>,
    tool_registry: ParkingMutex<Arc<ToolRegistry>>,
}''',
    '''struct AgentServices {
    agent_loop: ParkingMutex<Option<Arc<AgentLoop>>>,
    activation_service: Arc<ModelActivationService>,
    inference_backend_factory: ParkingMutex<Option<Arc<dyn InferenceBackendFactory>>>,
    chat_session: ParkingMutex<Option<(ConversationId, BranchId)>>,
    config: ParkingMutex<Option<MukeiConfig>>,
    tool_registry: ParkingMutex<Arc<ToolRegistry>>,
}''',
    "runtime activation ownership fields",
)
replace_once(
    "rust/crates/mukei-bridge/src/app_runtime.rs",
    '''            agent: AgentServices {
                agent_loop: ParkingMutex::new(None),
                chat_session: ParkingMutex::new(None),''',
    '''            agent: AgentServices {
                agent_loop: ParkingMutex::new(None),
                activation_service: ModelActivationService::new(false),
                inference_backend_factory: ParkingMutex::new(None),
                chat_session: ParkingMutex::new(None),''',
    "runtime activation ownership initialization",
)
replace_once(
    "rust/crates/mukei-bridge/src/app_runtime.rs",
    '''    pub(crate) fn set_agent_loop(&self, agent_loop: Arc<AgentLoop>) {
        *self.agent.agent_loop.lock() = Some(agent_loop);
    }

    pub(crate) fn chat_session(&self) -> Option<(ConversationId, BranchId)> {''',
    '''    pub(crate) fn set_agent_loop(&self, agent_loop: Arc<AgentLoop>) {
        *self.agent.agent_loop.lock() = Some(agent_loop);
    }

    pub(crate) fn model_activation_service(&self) -> Arc<ModelActivationService> {
        self.agent.activation_service.clone()
    }

    pub(crate) fn inference_backend_factory(&self) -> Option<Arc<dyn InferenceBackendFactory>> {
        self.agent.inference_backend_factory.lock().clone()
    }

    pub(crate) fn set_inference_backend_factory(
        &self,
        factory: Option<Arc<dyn InferenceBackendFactory>>,
    ) {
        let available = factory.is_some();
        *self.agent.inference_backend_factory.lock() = factory;
        self.agent
            .activation_service
            .set_real_backend_implementation_available(available);
    }

    pub(crate) fn chat_session(&self) -> Option<(ConversationId, BranchId)> {''',
    "runtime activation accessors",
)
replace_once(
    "rust/crates/mukei-bridge/src/app_runtime.rs",
    '''        assert!(Arc::ptr_eq(
            application_runtime().runtime_coordinator(),
            application_runtime().runtime_coordinator()
        ));
    }''',
    '''        assert!(Arc::ptr_eq(
            application_runtime().runtime_coordinator(),
            application_runtime().runtime_coordinator()
        ));
        let first_activation = application_runtime().model_activation_service();
        let second_activation = application_runtime().model_activation_service();
        assert!(Arc::ptr_eq(&first_activation, &second_activation));
        assert!(application_runtime().inference_backend_factory().is_none());
    }''',
    "runtime singleton activation test",
)

# ---------------------------------------------------------------------------
# AgentLoop composition uses process-owned activation router.
# ---------------------------------------------------------------------------
replace_once(
    "rust/crates/mukei-bridge/src/agent_runtime.rs",
    '''use mukei_core::engine::{BackendUnavailableReason, InferenceBackend, UnavailableInferenceBackend};''',
    '''use mukei_core::engine::{InferenceBackend, ModelActivationService};''',
    "agent runtime activation imports",
)
replace_once(
    "rust/crates/mukei-bridge/src/agent_runtime.rs",
    '''/// Build the shared `Arc<AgentLoop>` from loaded config, rebuilt tool
/// registry, and optional database pool. This compatibility assembly is
/// intentionally fail-closed until a production activation path injects a
/// runnable backend; it never selects the development mock implicitly.
#[cfg(feature = "rusqlite")]
pub fn build_agent_loop(
    cfg: &MukeiConfig,
    registry: Arc<ToolRegistry>,
    pool: Arc<mukei_core::storage::DatabasePool>,
    audit_writer: Arc<mukei_core::storage::AuditLogWriter>,
) -> Arc<AgentLoop> {
    tracing::warn!(
        backend_kind = "unavailable",
        "agent runtime built without an activated production inference backend"
    );
    build_agent_loop_with_backend(
        cfg,
        registry,
        pool,
        audit_writer,
        Arc::new(UnavailableInferenceBackend::new_with_reason(
            "production_backend_not_activated",
            BackendUnavailableReason::NotInjected,
        )),
    )
}''',
    '''/// Build the shared `Arc<AgentLoop>` around the process-owned activation
/// router. The loop remains stable while verified model backends are swapped
/// atomically by `ModelActivationService`.
#[cfg(feature = "rusqlite")]
pub fn build_agent_loop(
    cfg: &MukeiConfig,
    registry: Arc<ToolRegistry>,
    pool: Arc<mukei_core::storage::DatabasePool>,
    audit_writer: Arc<mukei_core::storage::AuditLogWriter>,
    activation_service: Arc<ModelActivationService>,
) -> Arc<AgentLoop> {
    tracing::info!(
        backend_kind = activation_service.identity().kind.as_tag(),
        "agent runtime wired to process-owned model activation service"
    );
    build_agent_loop_with_backend(cfg, registry, pool, audit_writer, activation_service)
}''',
    "rusqlite AgentLoop activation-router composition",
)
replace_once(
    "rust/crates/mukei-bridge/src/agent_runtime.rs",
    '''#[cfg(not(feature = "rusqlite"))]
pub fn build_agent_loop(cfg: &MukeiConfig, registry: Arc<ToolRegistry>) -> Arc<AgentLoop> {
    tracing::warn!(
        backend_kind = "unavailable",
        "agent runtime built without an activated production inference backend"
    );
    build_agent_loop_with_backend(
        cfg,
        registry,
        Arc::new(UnavailableInferenceBackend::new_with_reason(
            "production_backend_not_activated",
            BackendUnavailableReason::NotInjected,
        )),
    )
}''',
    '''#[cfg(not(feature = "rusqlite"))]
pub fn build_agent_loop(
    cfg: &MukeiConfig,
    registry: Arc<ToolRegistry>,
    activation_service: Arc<ModelActivationService>,
) -> Arc<AgentLoop> {
    tracing::info!(
        backend_kind = activation_service.identity().kind.as_tag(),
        "agent runtime wired to process-owned model activation service"
    );
    build_agent_loop_with_backend(cfg, registry, activation_service)
}''',
    "non-rusqlite AgentLoop activation-router composition",
)

# ---------------------------------------------------------------------------
# Bridge capability truth + boot composition + direct-path guard + diagnostics.
# ---------------------------------------------------------------------------
replace_once(
    "rust/crates/mukei-bridge/src/lib.rs",
    '''fn production_safety_status() -> mukei_core::config::ProductionSafetyStatus {
    mukei_core::config::ProductionSafetyStatus {''',
    '''fn current_ready_capabilities() -> CapabilitySnapshot {
    CapabilitySnapshot::ready_with_model(
        runtime_state()
            .model_activation_service()
            .readiness_snapshot()
            .active_backend_ready,
    )
}

fn production_safety_status() -> mukei_core::config::ProductionSafetyStatus {
    mukei_core::config::ProductionSafetyStatus {''',
    "bridge current capability helper",
)

# Replace bridge-level static ready projections with runtime-truth projection.
lib_path = Path("rust/crates/mukei-bridge/src/lib.rs")
lib_text = lib_path.read_text(encoding="utf-8")
ready_count = lib_text.count("CapabilitySnapshot::ready()")
if ready_count < 1:
    raise SystemExit("bridge capability replacement found no ready snapshots")
lib_text = lib_text.replace("CapabilitySnapshot::ready()", "current_ready_capabilities()")
lib_path.write_text(lib_text, encoding="utf-8")
print(f"PASS bridge capability truth replacement ({ready_count} call sites)")

replace_once(
    "rust/crates/mukei-bridge/src/lib.rs",
    '''                let loop_handle = agent_runtime::build_agent_loop(
                    &cfg,
                    registry_arc,
                    pool.clone(),
                    runtime_state().audit_log_writer().clone(),
                );''',
    '''                let loop_handle = agent_runtime::build_agent_loop(
                    &cfg,
                    registry_arc,
                    pool.clone(),
                    runtime_state().audit_log_writer().clone(),
                    runtime_state().model_activation_service(),
                );''',
    "rusqlite boot injects activation router",
)
replace_once(
    "rust/crates/mukei-bridge/src/lib.rs",
    '''                let registry_arc = runtime_state().tool_registry();
                let loop_handle = agent_runtime::build_agent_loop(&cfg, registry_arc);
                runtime_state().set_agent_loop(loop_handle);''',
    '''                let registry_arc = runtime_state().tool_registry();
                let loop_handle = agent_runtime::build_agent_loop(
                    &cfg,
                    registry_arc,
                    runtime_state().model_activation_service(),
                );
                runtime_state().set_agent_loop(loop_handle);''',
    "non-rusqlite boot injects activation router",
)
replace_once(
    "rust/crates/mukei-bridge/src/lib.rs",
    '''        let input = user_input.to_string();
        let (conversation_id, branch_id) = match runtime_state().chat_session() {''',
    '''        if !runtime_state()
            .model_activation_service()
            .readiness_snapshot()
            .active_backend_ready
        {
            let err = mukei_core::error::MukeiError::ModelLoadFailed(
                "no active production inference backend".to_string(),
            );
            let event = error_bridge_event(&err, "send_message");
            let code = err.error_code().to_string();
            let message = mukei_core::diagnostics::sanitize_error_message(err.to_string());
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().event_emitted(event_json(event));
                qobject
                    .as_mut()
                    .error_occurred(QString::from(&code), QString::from(&message));
            });
            return;
        }
        let input = user_input.to_string();
        let (conversation_id, branch_id) = match runtime_state().chat_session() {''',
    "legacy direct chat path fails closed without active backend",
)
replace_once(
    "rust/crates/mukei-bridge/src/lib.rs",
    '''        let restart_required = selected_model_id.is_some();
        QString::from(serde_json::json!({
            "schema_version": 1,
            "selected_model_id": selected_model_id,
            "loaded_model_id": serde_json::Value::Null,
            "inference_backend": "mock_unwired",
            "activation_supported": false,
            "restart_required": restart_required,
            "safe_message": "The selected model is stored for a future engine session. Live llama.cpp activation is not wired in this build."
        }).to_string().as_str())''',
    '''        let activation = runtime_state().model_activation_service();
        let readiness = activation.readiness_snapshot();
        let active = activation.active_model_snapshot();
        let identity = activation.identity();
        let loaded_model_id = active.as_ref().map(|snapshot| snapshot.model_id.clone());
        let activation_required = selected_model_id.as_ref() != loaded_model_id.as_ref();
        let safe_message = if readiness.active_backend_ready {
            "The active model is ready for local inference."
        } else if !readiness.real_backend_implementation_available {
            "A production inference backend factory is not registered in this runtime."
        } else {
            "Select and activate an installed model before starting chat."
        };
        QString::from(serde_json::json!({
            "schema_version": 2,
            "selected_model_id": selected_model_id,
            "loaded_model_id": loaded_model_id,
            "inference_backend": identity.implementation,
            "backend_kind": identity.kind.as_tag(),
            "backend_unavailable_reason": identity.unavailable_reason.map(|reason| reason.as_tag()),
            "activation_supported": readiness.real_backend_implementation_available,
            "activation_required": activation_required,
            "active_model_ready": readiness.active_backend_ready,
            "product_ready": readiness.product_ready,
            "restart_required": false,
            "safe_message": safe_message
        }).to_string().as_str())''',
    "engine session reports activation truth",
)

# ---------------------------------------------------------------------------
# Protocol V2 preflight rejects chat/recovery before acknowledgement when no
# active backend exists.
# ---------------------------------------------------------------------------
replace_once(
    "rust/crates/mukei-bridge/src/protocol.rs",
    '''        CommandType::ChatSendMessage => {
            if agent
                .as_ref()''',
    '''        CommandType::ChatSendMessage => {
            if !runtime_state()
                .model_activation_service()
                .readiness_snapshot()
                .active_backend_ready
            {
                return Some(RejectionReason::CapabilityUnavailable);
            }
            if agent
                .as_ref()''',
    "Protocol V2 chat preflight requires active backend",
)
replace_once(
    "rust/crates/mukei-bridge/src/protocol.rs",
    '''        CommandType::RecoveryResume | CommandType::RecoveryRegenerate => {
            if agent
                .as_ref()''',
    '''        CommandType::RecoveryResume | CommandType::RecoveryRegenerate => {
            if !runtime_state()
                .model_activation_service()
                .readiness_snapshot()
                .active_backend_ready
            {
                return Some(RejectionReason::CapabilityUnavailable);
            }
            if agent
                .as_ref()''',
    "Protocol V2 recovery preflight requires active backend",
)

# ---------------------------------------------------------------------------
# Add focused capability tests and remove temporary patch transport.
# ---------------------------------------------------------------------------
ui_path = Path("rust/crates/mukei-core/src/ui_contract.rs")
ui_text = ui_path.read_text(encoding="utf-8")
marker = '''    #[test]
    fn bridge_event_serializes_stable_snake_case_envelope() {'''
if ui_text.count(marker) != 1:
    raise SystemExit("ui capability test insertion anchor mismatch")
insert = '''    #[test]
    fn ready_capabilities_fail_closed_without_active_model() {
        let capabilities = CapabilitySnapshot::ready();
        assert!(!capabilities.can_send_message);
        assert!(!capabilities.active_model_ready);
    }

    #[test]
    fn ready_capabilities_enable_chat_only_with_active_model() {
        let capabilities = CapabilitySnapshot::ready_with_model(true);
        assert!(capabilities.can_send_message);
        assert!(capabilities.active_model_ready);
    }

    #[test]
    fn inferencing_capabilities_require_an_active_model() {
        let capabilities = CapabilitySnapshot::inferencing();
        assert!(capabilities.active_model_ready);
        assert!(capabilities.is_inferencing);
    }

'''
ui_path.write_text(ui_text.replace(marker, insert + marker, 1), encoding="utf-8")
print("PASS capability truth tests")

for temporary in [
    ".github/patch_activation_composition_root.py",
    ".github/workflows/activation-composition-root-runner.yml",
]:
    candidate = Path(temporary)
    if candidate.exists():
        candidate.unlink()
        print(f"PASS removed temporary {temporary}")

print("Activation composition-root wiring complete")
