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
        let runtime_handle = async_runtime.handle().clone();
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
            features: Arc::new(FeatureState::new(runtime_handle)),
            settings: RwLock::new(HashMap::new()),
            replay: Mutex::new(HashMap::new()),
            product_config: RwLock::new(None),
            activation,
            backend_factory: services.backend_factory,
            agent_loop: RwLock::new(None),
            projection_store: RwLock::new(None),
            rag_service: RwLock::new(None),
            #[cfg(feature = "rusqlite")]
            storage_importer: RwLock::new(services.storage_importer),
            #[cfg(feature = "rusqlite")]
            storage_workspace: RwLock::new(services.storage_workspace),
            #[cfg(feature = "rusqlite")]
            conversation_attachments: RwLock::new(services.conversation_attachments),
            remote_tool_secrets: Mutex::new(None),
            remote_policy: RwLock::new(crate::tools::RemoteFeaturePolicy::LocalOnly),
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
            CommandType::ConversationRename,
            CommandType::ConversationArchive,
            CommandType::ConversationDelete,
            CommandType::ConversationSelectBranch,
            CommandType::DownloadCancel,
            CommandType::ModelDelete,
            CommandType::DocumentGrant,
            CommandType::DocumentRevoke,
            CommandType::ProjectCreate,
            CommandType::ProjectUpdate,
            CommandType::ProjectArchive,
            CommandType::ProjectInstructionsUpdate,
    CommandType::ProjectMemoryAdd,
    CommandType::ProjectMemoryUpdate,
    CommandType::ProjectMemoryDelete,
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
        if self
            .rag_service
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
        {
            commands.push(CommandType::DocumentRetryIngestion);
        }
        #[cfg(feature = "network")]
        commands.push(CommandType::ModelDownload);
        #[cfg(feature = "rusqlite")]
        if self
            .storage_importer
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
        {
            commands.push(CommandType::StorageImportFile);
        }
        #[cfg(feature = "rusqlite")]
        if self
            .storage_workspace
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
        {
            commands.extend([
                CommandType::StorageDirectoryCreate,
                CommandType::StorageNodeRename,
                CommandType::StorageNodeTrash,
                CommandType::StorageNodeRestore,
            ]);
        }
        #[cfg(feature = "rusqlite")]
        if self
            .conversation_attachments
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
        {
            commands.extend([
                CommandType::ConversationAttachmentAdd,
                CommandType::ConversationAttachmentRemove,
            ]);
        }
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
        let expected_config_path = root.join("mukei.toml");
        if Path::new(&payload.config_path) != expected_config_path.as_path() {
            *self.state.write().unwrap_or_else(|p| p.into_inner()) = RuntimeState::Failed;
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::PolicyDenied,
            );
        }
        if !expected_config_path.is_file()
            && crate::config::write_default(&expected_config_path).is_err()
        {
            *self.state.write().unwrap_or_else(|p| p.into_inner()) = RuntimeState::Failed;
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::BackendUnavailable,
            );
        }
        let product_config = match MukeiConfig::load_and_validate(&expected_config_path) {
            Ok(config) => config,
            Err(_) => {
                *self.state.write().unwrap_or_else(|p| p.into_inner()) = RuntimeState::Failed;
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::InvalidPayload,
                );
            }
        };
        if product_config
            .validate_android_storage_paths(&expected_config_path)
            .is_err()
            || product_config.ensure_storage_directories().is_err()
        {
            *self.state.write().unwrap_or_else(|p| p.into_inner()) = RuntimeState::Failed;
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::BackendUnavailable,
            );
        }

        self.install_agent_loop(&product_config);
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
