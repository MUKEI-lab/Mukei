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

    fn build_agent_loop(
        &self,
        product_config: &MukeiConfig,
        rag_service: Option<Arc<dyn RuntimeRagService>>,
        temporary: bool,
    ) -> Arc<AgentLoop> {
        let context = ContextBudgetManager::new(
            Arc::new(RuntimeContextBackend {
                features: Arc::clone(&self.features),
                ephemeral_chats: Arc::clone(&self.ephemeral_chats),
                rag_service,
            }),
            Arc::new(RuntimeTokenCounter),
            product_config.n_ctx,
        );

        let registry = if temporary {
            ToolRegistry::temporary_chat()
        } else {
            let remote_policy = *self
                .remote_policy
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        AgentLoop::new_with_backend(context, executor, watchdog, backend)
    }

    fn temporary_agent_loop(&self) -> Option<Arc<AgentLoop>> {
        let config = self
            .product_config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()?;
        Some(self.build_agent_loop(&config, None, true))
    }

    fn install_agent_loop(&self, product_config: &MukeiConfig) {
        let rag_service = self
            .rag_service
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let agent_loop = self.build_agent_loop(product_config, rag_service, false);
        *self
            .agent_loop
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(agent_loop);
    }
}

#[cfg(test)]
mod temporary_chat_rag_tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingRag {
        retrievals: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl RuntimeRagService for CountingRag {
        async fn ingest_document(
            &self,
            _document_id: &str,
            _staged_path: &Path,
            _mime_type: &str,
        ) -> Result<RagIngestResult, MukeiError> {
            Ok(RagIngestResult { chunk_count: 0 })
        }

        async fn retrieve(&self, _query: &str, _top_k: usize) -> Result<Vec<String>, MukeiError> {
            self.retrievals.fetch_add(1, Ordering::AcqRel);
            Ok(vec!["normal-rag-only".to_string()])
        }

        async fn revoke_document(&self, _document_id: &str) -> Result<usize, MukeiError> {
            Ok(0)
        }
    }

    #[test]
    fn temporary_agent_context_never_calls_attached_rag_service() {
        let runtime = MukeiRuntime::create(RuntimeConfig {
            app_data_dir: format!("/tmp/mukei-temp-rag-test-{}", Uuid::new_v4()),
            worker_threads: 1,
            max_blocking_threads: 2,
            event_capacity: 64,
        })
        .expect("runtime");
        let product = MukeiConfig::default_for_data_root(Path::new(&runtime.config.app_data_dir));
        let rag = Arc::new(CountingRag {
            retrievals: AtomicUsize::new(0),
        });
        let normal = runtime.build_agent_loop(&product, Some(rag.clone()), false);
        let temporary = runtime.build_agent_loop(&product, None, true);
        let conversation = ConversationId::new();
        let branch = BranchId::new();
        let history = vec![ChatMessage::user_with_id(MessageId::new(), branch, "find my notes")];

        runtime
            .async_runtime
            .block_on(normal.context.build_for(conversation, branch, &history))
            .expect("normal context");
        assert_eq!(rag.retrievals.load(Ordering::Acquire), 1);

        runtime
            .async_runtime
            .block_on(temporary.context.build_for(conversation, branch, &history))
            .expect("temporary context");
        assert_eq!(rag.retrievals.load(Ordering::Acquire), 1);
        assert_eq!(
            temporary.tools.registry_names(),
            vec!["get_hardware_info".to_string(), "math_eval".to_string()]
        );
    }
}
