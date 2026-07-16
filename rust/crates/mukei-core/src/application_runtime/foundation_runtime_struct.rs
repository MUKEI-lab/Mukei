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
    closed: AtomicBool,
}
