impl MukeiRuntime {
    /// Allocate a runtime without production backend services.
    pub fn create(config: RuntimeConfig) -> Result<Self, RuntimeError> {
        Self::create_with_services(config, RuntimeServices::default())
    }

    /// Allocate a runtime with explicitly installed native services.
    pub fn create_with_services(
        config: RuntimeConfig,
        services: RuntimeServices,
    ) -> Result<Self, RuntimeError> {
        config.validate()?;
        let async_runtime = Builder::new_multi_thread()
            .worker_threads(config.worker_threads)
            .max_blocking_threads(config.max_blocking_threads)
            .thread_name("mukei-native")
            .enable_all()
            .build()?;
        let events = Arc::new(EventBus::new(config.event_capacity));
        let activation = ModelActivationService::new(services.backend_factory.is_some());
        let runtime = Self {
            session_id: Uuid::new_v4().to_string(),
            config,
            state: RwLock::new(RuntimeState::Created),
            async_runtime,
            cancellation: CancellationToken::new(),
            events,
            platform: Arc::new(PlatformRequestBroker::default()),
            features: Arc::new(FeatureState::default()),
            settings: RwLock::new(HashMap::new()),
            replay: Mutex::new(HashMap::new()),
            product_config: RwLock::new(None),
            activation,
            backend_factory: services.backend_factory,
            agent_loop: RwLock::new(None),
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

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn state(&self) -> RuntimeState {
        *self
            .state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Protocol capabilities genuinely implemented by this runtime.
    pub fn capabilities(&self) -> ProtocolCapabilitySnapshot {
        let mut commands = vec![
            CommandType::AppInitialize,
            CommandType::ChatClearConversation,
            CommandType::DownloadCancel,
            CommandType::ModelDelete,
            CommandType::DocumentGrant,
            CommandType::DocumentRevoke,
            CommandType::SettingsUpdate,
        ];
        if self.backend_factory.is_some() {
            commands.extend([
                CommandType::ChatSendMessage,
                CommandType::ChatStopGeneration,
                CommandType::ModelSelect,
                CommandType::RecoveryResume,
                CommandType::RecoveryRegenerate,
            ]);
        }
        #[cfg(feature = "network")]
        commands.push(CommandType::ModelDownload);
        ProtocolCapabilitySnapshot::for_commands(&commands)
            .with_transport(CAP_EVENT_GAP_REPORTING)
            .with_transport(CAP_PLATFORM_REQUEST_BROKER)
            .with_transport(CAP_ANDROID_DOCUMENT_PORT)
            .with_transport(CAP_ANDROID_KEYSTORE_PORT)
    }

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

    fn ensure_ready(&self, command: &ValidatedCommand) -> Result<(), CommandAcknowledgementV2> {
        if self.state() != RuntimeState::Ready {
            Err(CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::BackendUnavailable,
            ))
        } else {
            Ok(())
        }
    }

    fn accept_operation(
        &self,
        command: &ValidatedCommand,
    ) -> (CommandAcknowledgementV2, String, CancellationToken) {
        let (operation_id, token) = self.features.create_operation(command);
        self.events.emit(
            &format!("operation:{operation_id}"),
            "operation.accepted",
            json!({"state": "accepted"}),
            Some(&command.envelope),
            Some(operation_id.clone()),
        );
        (
            CommandAcknowledgementV2::accepted(&command.envelope, Some(operation_id.clone())),
            operation_id,
            token,
        )
    }

    fn initialize(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        let ValidatedCommandPayload::Initialize(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
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

        let root = PathBuf::from(&self.config.app_data_dir);
        let product_config = if Path::new(&payload.config_path).is_file() {
            match MukeiConfig::load_and_validate(Path::new(&payload.config_path)) {
                Ok(config) => config,
                Err(_) => {
                    *self
                        .state
                        .write()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = RuntimeState::Failed;
                    return CommandAcknowledgementV2::rejected(
                        Some(&command.envelope),
                        RejectionReason::InvalidPayload,
                    );
                }
            }
        } else {
            MukeiConfig::default_for_data_root(&root)
        };
        if product_config.ensure_storage_directories().is_err() {
            *self
                .state
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = RuntimeState::Failed;
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::BackendUnavailable,
            );
        }

        let context = ContextBudgetManager::new(
            Arc::new(RuntimeContextBackend {
                features: Arc::clone(&self.features),
            }),
            Arc::new(RuntimeTokenCounter),
            product_config.n_ctx,
        );
        let policy = ToolExecutionPolicy::from(&product_config.agent);
        let executor = ToolExecutor::with_policy(
            Arc::new(ToolRegistry::new()),
            Arc::new(FailureTracker::with_threshold(
                product_config.agent.max_failures_per_tool,
            )),
            policy,
        );
        let watchdog = WatchdogHandle::new(Watchdog::new(
            product_config.watchdog.max_iterations,
            product_config.watchdog.max_token_budget,
            Duration::from_secs(product_config.watchdog.max_wall_seconds),
        ));
        let backend: Arc<dyn crate::engine::InferenceBackend> = self.activation.clone();
        let agent_loop = AgentLoop::new_with_backend(context, executor, watchdog, backend);
        *self
            .agent_loop
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(agent_loop);
        *self
            .product_config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(product_config);

        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        *self
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = RuntimeState::Ready;
        self.features.update_operation(
            &operation_id,
            OperationStatus::Completed,
            Some(1.0),
            None,
            json!({"runtime_session_id": self.session_id}),
        );
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
            json!({"state": "completed"}),
            Some(&command.envelope),
            Some(operation_id),
        );
        acknowledgement
    }
}
