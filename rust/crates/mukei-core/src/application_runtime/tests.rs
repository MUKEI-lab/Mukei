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

    fn runtime() -> MukeiRuntime {
        MukeiRuntime::create(RuntimeConfig {
            app_data_dir: "/tmp/mukei-runtime-tests".into(),
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
        let acknowledgement = runtime.submit(command(
            "app.initialize",
            json!({"config_path": "/tmp/mukei-runtime-tests/missing.toml"}),
        ));
        assert_eq!(acknowledgement.status, AcknowledgementStatus::Accepted);
        assert_eq!(runtime.state(), RuntimeState::Ready);
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
        assert!(matches!(batch.requests[0].request, PlatformRequestKind::DocumentStage { .. }));
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
