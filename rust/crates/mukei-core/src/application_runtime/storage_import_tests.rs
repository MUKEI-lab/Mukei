#[cfg(all(test, feature = "rusqlite"))]
mod storage_import_runtime_tests {
    use super::*;
    use crate::storage::{
        ImportTransactionId, StagedFileImporter, StagedImportError, StorageNodeId,
        StorageObjectId, WorkspaceStagedImportReceipt, WorkspaceStagedImportRequest,
    };
    use std::thread;

    #[derive(Default)]
    struct RecordingImporter {
        request: Mutex<Option<WorkspaceStagedImportRequest>>,
    }

    #[async_trait::async_trait]
    impl StagedFileImporter for RecordingImporter {
        async fn import_workspace_file(
            &self,
            request: WorkspaceStagedImportRequest,
            _cancellation: CancellationToken,
        ) -> Result<WorkspaceStagedImportReceipt, StagedImportError> {
            *self
                .request
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(request);
            Ok(WorkspaceStagedImportReceipt {
                transaction_id: ImportTransactionId::new(),
                node_id: StorageNodeId::new(),
                object_id: StorageObjectId::new(),
                display_name: "notes.txt".to_string(),
                plaintext_size: 12,
                deduplicated: false,
                staged_file_removed: true,
            })
        }
    }

    fn storage_command() -> CommandEnvelopeV2 {
        CommandEnvelopeV2 {
            protocol_version: crate::ui_protocol::ProtocolVersion::CURRENT,
            command_id: "cmd-storage-import".into(),
            request_id: "req-storage-import".into(),
            command_type: "storage.import_file".into(),
            submitted_at: Utc::now(),
            operation_id: None,
            correlation_id: "corr-storage-import".into(),
            idempotency_key: Some("idem-storage-import".into()),
            scope: Some(crate::ui_protocol::CommandScope {
                conversation_id: Some("chat-1".into()),
                branch_id: Some("branch-1".into()),
                turn_id: None,
                model_id: None,
                document_id: None,
            }),
            payload: json!({
                "target": "content://documents/notes",
                "display_name": "notes.txt",
                "mime_type": "text/plain"
            }),
        }
    }

    fn ready_runtime(
        importer: Option<Arc<dyn StagedFileImporter>>,
    ) -> (tempfile::TempDir, MukeiRuntime) {
        let directory = tempfile::tempdir().unwrap();
        let runtime = MukeiRuntime::create_with_services(
            RuntimeConfig {
                app_data_dir: directory.path().to_string_lossy().into_owned(),
                worker_threads: 2,
                max_blocking_threads: 4,
                event_capacity: 128,
            },
            RuntimeServices {
                backend_factory: None,
                storage_importer: importer,
                storage_workspace: None,
                conversation_attachments: None,
            },
        )
        .unwrap();
        *runtime
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = RuntimeState::Ready;
        (directory, runtime)
    }

    #[test]
    fn capability_is_truthful_when_importer_is_absent() {
        let (_directory, runtime) = ready_runtime(None);
        assert!(!runtime
            .capabilities()
            .capabilities
            .contains(&"command:storage.import_file".to_string()));
        let acknowledgement = runtime.submit(storage_command());
        assert_eq!(acknowledgement.status, AcknowledgementStatus::Rejected);
        assert_eq!(
            acknowledgement.rejection_reason,
            Some(RejectionReason::CapabilityUnavailable)
        );
        runtime.shutdown();
    }

    #[test]
    fn staged_platform_response_is_forwarded_to_workspace_importer() {
        let importer = Arc::new(RecordingImporter::default());
        let (_directory, runtime) = ready_runtime(Some(importer.clone()));
        assert!(runtime
            .capabilities()
            .capabilities
            .contains(&"command:storage.import_file".to_string()));

        let acknowledgement = runtime.submit(storage_command());
        assert_eq!(acknowledgement.status, AcknowledgementStatus::Accepted);
        let batch = runtime.platform.drain(1, Duration::ZERO);
        assert_eq!(batch.requests.len(), 1);
        let request = &batch.requests[0];
        assert!(matches!(
            request.request,
            PlatformRequestKind::DocumentStage { .. }
        ));
        runtime
            .platform
            .submit_response(PlatformResponse {
                request_id: request.request_id.clone(),
                status: crate::platform::PlatformResponseStatus::Succeeded,
                payload: json!({
                    "staged_path": "/data/user/0/ai.mukei.android/files/staging/notes.txt",
                    "size_bytes": 12
                }),
                error_code: None,
                error_message: None,
            })
            .unwrap();

        for _ in 0..100 {
            if importer
                .request
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_some()
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let captured = importer
            .request
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .expect("runtime should call the staged-file importer");
        assert_eq!(captured.chat_id.as_str(), "chat-1");
        assert_eq!(captured.original_filename, "notes.txt");
        assert_eq!(captured.expected_size, Some(12));

        let events = runtime.events.drain(64, Duration::from_millis(100));
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == "storage.file_imported"));
        runtime.shutdown();
    }
}
