impl Drop for MukeiRuntime {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.cancellation.cancel();
        self.features.cancel_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_protocol::{CommandScope, ProtocolVersion};

    struct DelayedProjectionStore {
        writes: std::sync::Mutex<Vec<Value>>,
        delay_first: AtomicBool,
    }

    impl DelayedProjectionStore {
        fn new() -> Self {
            Self {
                writes: std::sync::Mutex::new(Vec::new()),
                delay_first: AtomicBool::new(true),
            }
        }
    }

    #[async_trait::async_trait]
    impl RuntimeProjectionStore for DelayedProjectionStore {
        async fn load(&self, _key: &str) -> Result<Option<Value>, MukeiError> {
            Ok(None)
        }

        async fn save(&self, _key: &str, value: Value) -> Result<(), MukeiError> {
            if self
                .delay_first
                .swap(false, std::sync::atomic::Ordering::AcqRel)
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            self.writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(value);
            Ok(())
        }

        async fn delete(&self, _key: &str) -> Result<(), MukeiError> {
            Ok(())
        }
    }

    fn runtime() -> MukeiRuntime {
        MukeiRuntime::create(RuntimeConfig {
            app_data_dir: format!("/tmp/mukei-runtime-tests-{}", Uuid::new_v4()),
            worker_threads: 1,
            max_blocking_threads: 2,
            event_capacity: 64,
        })
        .expect("runtime")
    }

    fn command(command_type: &str, payload: Value) -> CommandEnvelopeV2 {
        CommandEnvelopeV2 {
            protocol_version: ProtocolVersion::CURRENT,
            command_id: Uuid::new_v4().to_string(),
            request_id: Uuid::new_v4().to_string(),
            command_type: command_type.into(),
            submitted_at: Utc::now(),
            operation_id: None,
            correlation_id: Uuid::new_v4().to_string(),
            idempotency_key: None,
            scope: None,
            payload,
        }
    }

    fn initialize(runtime: &MukeiRuntime) {
        let config_path = Path::new(&runtime.config.app_data_dir).join("mukei.toml");
        let acknowledgement = runtime.submit(command(
            "app.initialize",
            json!({"config_path": config_path.to_string_lossy()}),
        ));
        assert_eq!(acknowledgement.status, AcknowledgementStatus::Accepted);
        assert_eq!(runtime.state(), RuntimeState::Ready);
    }

    #[test]
    fn projection_writer_preserves_fifo_order() {
        let runtime = runtime();
        let store = Arc::new(DelayedProjectionStore::new());
        runtime.features.attach_projection_store(store.clone());

        runtime
            .features
            .persist_value("operations", json!({"revision": 1}));
        runtime
            .features
            .persist_value("operations", json!({"revision": 2}));

        let (barrier_sender, barrier_receiver) = tokio::sync::oneshot::channel();
        assert!(runtime
            .features
            .persistence_sender
            .send(PersistenceCommand::Barrier(barrier_sender))
            .is_ok());
        runtime
            .async_runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(2), barrier_receiver).await
            })
            .expect("projection barrier timeout")
            .expect("projection barrier");

        let writes = store
            .writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            writes.as_slice(),
            &[json!({"revision": 1}), json!({"revision": 2})]
        );
    }

    #[test]
    fn capabilities_advertise_feature_handlers_and_platform_ports() {
        let capabilities = runtime().capabilities().capabilities;
        assert!(!capabilities.contains(&"command:chat.send_message".to_string()));
        assert!(capabilities.contains(&"command:document.grant".to_string()));
        assert!(capabilities.contains(&CAP_PLATFORM_REQUEST_BROKER.to_string()));
    }

    #[test]
    fn chat_fails_closed_without_active_model() {
        let runtime = runtime();
        initialize(&runtime);
        let mut envelope = command("chat.send_message", json!({"text": "hello"}));
        envelope.idempotency_key = Some("chat-one".into());
        envelope.scope = Some(CommandScope {
            conversation_id: Some(Uuid::new_v4().to_string()),
            branch_id: Some(Uuid::new_v4().to_string()),
            ..CommandScope::default()
        });
        let acknowledgement = runtime.submit(envelope);
        assert_eq!(
            acknowledgement.rejection_reason,
            Some(RejectionReason::BackendUnavailable)
        );
    }

    #[test]
    fn document_grant_queues_android_platform_request() {
        let runtime = runtime();
        initialize(&runtime);
        let mut envelope = command(
            "document.grant",
            json!({"target": "content://documents/1", "label": "one.pdf", "mime_type": "application/pdf"}),
        );
        envelope.idempotency_key = Some("document-one".into());
        let acknowledgement = runtime.submit(envelope);
        assert_eq!(acknowledgement.status, AcknowledgementStatus::Accepted);
        let batch = runtime.drain_platform_requests(4, Duration::ZERO);
        assert_eq!(batch.requests.len(), 1);
        assert!(matches!(
            batch.requests[0].request,
            PlatformRequestKind::DocumentStage { .. }
        ));
    }

    #[test]
    fn shutdown_is_idempotent_and_application_snapshot_remains_available() {
        let runtime = runtime();
        runtime.shutdown();
        runtime.shutdown();
        assert_eq!(runtime.state(), RuntimeState::Stopped);
        assert!(runtime.snapshot(RuntimeSnapshotDomain::Application).is_ok());
    }
}
