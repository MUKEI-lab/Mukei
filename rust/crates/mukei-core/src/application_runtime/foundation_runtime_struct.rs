/// Process-scoped Mukei application runtime.
pub struct MukeiRuntime {
    session_id: String,
    config: RuntimeConfig,
    state: RwLock<RuntimeState>,
    async_runtime: Runtime,
    cancellation: CancellationToken,
    events: Arc<EventBus>,
    platform: Arc<PlatformRequestBroker>,
    features: Arc<FeatureState>,
    settings: RwLock<HashMap<String, Value>>,
    replay: Mutex<HashMap<String, ReplayRecord>>,
    product_config: RwLock<Option<MukeiConfig>>,
    activation: Arc<ModelActivationService>,
    backend_factory: Option<Arc<dyn InferenceBackendFactory>>,
    agent_loop: RwLock<Option<Arc<AgentLoop>>>,
    projection_store: RwLock<Option<Arc<dyn RuntimeProjectionStore>>>,
    rag_service: RwLock<Option<Arc<dyn RuntimeRagService>>>,
    #[cfg(feature = "rusqlite")]
    storage_importer: RwLock<Option<Arc<dyn crate::storage::StagedFileImporter>>>,
    #[cfg(feature = "rusqlite")]
    storage_workspace: RwLock<Option<Arc<dyn crate::storage::StorageWorkspacePort>>>,
    #[cfg(feature = "rusqlite")]
    conversation_attachments: RwLock<Option<Arc<dyn crate::storage::ConversationAttachmentPort>>>,
    remote_tool_secrets: Mutex<Option<RemoteToolSecrets>>,
    remote_policy: RwLock<crate::tools::RemoteFeaturePolicy>,
    closed: AtomicBool,
}
