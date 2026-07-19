impl MukeiRuntime {
    /// Attach the encrypted authoritative projection store before initialization.
    pub fn attach_projection_store(
        &self,
        store: Arc<dyn RuntimeProjectionStore>,
    ) -> Result<(), MukeiError> {
        *self
            .projection_store
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::clone(&store));
        self.features.attach_projection_store(Arc::clone(&store));
        self.async_runtime
            .block_on(self.features.hydrate_from_store())?;
        if let Some(value) = self.async_runtime.block_on(store.load("settings"))? {
            let settings: HashMap<String, Value> = serde_json::from_value(value)
                .map_err(|_| MukeiError::DatabaseCorruption)?;
            if let Some(policy) = settings
                .get("remote_feature_policy")
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<crate::tools::RemoteFeaturePolicy>().ok())
            {
                *self
                    .remote_policy
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = policy;
            }
            *self
                .settings
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = settings;
        }
        Ok(())
    }

    /// Attach a production RAG service before protocol capability negotiation.
    pub fn attach_rag_service(&self, service: Arc<dyn RuntimeRagService>) {
        *self
            .rag_service
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(service);
        if let Some(config) = self
            .product_config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        {
            self.install_agent_loop(&config);
        }
    }

    /// Install transient, already-unwrapped provider credentials.
    pub fn configure_remote_tools(
        &self,
        brave_key: zeroize::Zeroizing<String>,
        tavily_key: zeroize::Zeroizing<String>,
    ) -> Result<(), MukeiError> {
        if brave_key.trim().is_empty() || tavily_key.trim().is_empty() {
            return Err(MukeiError::ConfigInvalid {
                field: "remote_tool_secrets".into(),
                reason: "provider credentials must be non-empty".into(),
            });
        }
        *self
            .remote_tool_secrets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(RemoteToolSecrets {
            brave_key,
            tavily_key,
        });
        if let Some(config) = self
            .product_config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        {
            self.install_agent_loop(&config);
        }
        Ok(())
    }

    fn set_remote_policy(&self, policy: crate::tools::RemoteFeaturePolicy) {
        *self
            .remote_policy
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = policy;
        if let Some(config) = self
            .product_config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        {
            self.install_agent_loop(&config);
        }
    }

    fn persist_settings_now(&self) -> Result<(), MukeiError> {
        let store = self
            .projection_store
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(store) = store else { return Ok(()); };
        let value = serde_json::to_value(
            self.settings
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        )
        .map_err(|error| MukeiError::Internal(error.to_string()))?;
        self.async_runtime.block_on(store.save("settings", value))
    }

    fn install_agent_loop(&self, product_config: &MukeiConfig) {
        let rag_service = self
            .rag_service
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let context = ContextBudgetManager::new(
            Arc::new(RuntimeContextBackend {
                features: Arc::clone(&self.features),
                ephemeral_chats: Arc::clone(&self.ephemeral_chats),
                rag_service,
            }),
            Arc::new(RuntimeTokenCounter),
            product_config.n_ctx,
        );

        let remote_policy = *self
            .remote_policy
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let registry = {
            let secrets = self
                .remote_tool_secrets
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match secrets.as_ref() {
                Some(secrets)
                    if matches!(remote_policy, crate::tools::RemoteFeaturePolicy::RemoteAllowed) =>
                {
                    ToolRegistry::with_web_search_secrets_and_policy(
                        zeroize::Zeroizing::new(secrets.brave_key.as_str().to_owned()),
                        zeroize::Zeroizing::new(secrets.tavily_key.as_str().to_owned()),
                        remote_policy,
                    )
                }
                _ => ToolRegistry::local_only(),
            }
        };

        let policy = ToolExecutionPolicy::from(&product_config.agent);
        let executor = ToolExecutor::with_policy(
            Arc::new(registry),
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
    }
}
