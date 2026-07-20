#[cfg(feature = "rusqlite")]
impl MukeiRuntime {
    fn import_storage_file(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(acknowledgement) = self.ensure_ready(command) {
            return acknowledgement;
        }
        let ValidatedCommandPayload::StorageImport(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        let universal_parent = payload
            .parent_node_id
            .as_deref()
            .and_then(parse_storage_node_id);
        let chat_id = if universal_parent.is_none() {
            let Some(conversation_id) = command
                .envelope
                .scope
                .as_ref()
                .and_then(|scope| scope.conversation_id.as_deref())
            else {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::StaleScope,
                );
            };
            match crate::storage::ChatId::parse(conversation_id) {
                Ok(value) => Some(value),
                Err(_) => {
                    return CommandAcknowledgementV2::rejected(
                        Some(&command.envelope),
                        RejectionReason::StaleScope,
                    )
                }
            }
        } else {
            None
        };
        let importer = self
            .storage_importer
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(importer) = importer else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::CapabilityUnavailable,
            );
        };

        let (acknowledgement, operation_id, cancellation) = self.accept_operation(command);
        let request_id = match self.platform.enqueue(
            Some(operation_id.clone()),
            PlatformRequestKind::DocumentStage {
                target: payload.target.clone(),
                label: payload.display_name.clone(),
                mime_type: payload.mime_type.clone(),
            },
        ) {
            Ok(value) => value,
            Err(_) => {
                self.features.update_operation(
                    &operation_id,
                    OperationStatus::Failed,
                    None,
                    Some("platform_queue_full".into()),
                    Value::Null,
                );
                self.events.emit(
                    &format!("operation:{operation_id}"),
                    "operation.failed",
                    json!({"code": "platform_queue_full"}),
                    Some(&command.envelope),
                    Some(operation_id),
                );
                return acknowledgement;
            }
        };

        let platform = Arc::clone(&self.platform);
        let features = Arc::clone(&self.features);
        let events = Arc::clone(&self.events);
        let command_envelope = command.envelope.clone();
        let operation_id_for_task = operation_id.clone();
        let display_name = payload.display_name.clone();
        let mime_type = payload.mime_type.clone();
        let source_fingerprint = blake3::hash(payload.target.as_bytes()).to_hex().to_string();
        self.async_runtime.handle().spawn(async move {
            let platform_payload = match platform
                .wait_for_response(
                    &request_id,
                    PLATFORM_WAIT_TIMEOUT,
                    cancellation.clone(),
                )
                .await
            {
                Ok(value) => value,
                Err(error) => {
                    let code = match error {
                        PlatformPortError::Cancelled => "storage_import_cancelled",
                        PlatformPortError::Timeout => "platform_document_stage_timeout",
                        PlatformPortError::QueueFull => "platform_queue_full",
                        PlatformPortError::UnknownRequest | PlatformPortError::Failed(_) => {
                            "platform_document_stage_failed"
                        }
                    };
                    fail_storage_import_operation(
                        &features,
                        &events,
                        &command_envelope,
                        &operation_id_for_task,
                        code,
                    );
                    return;
                }
            };
            let staged_path = platform_payload
                .get("staged_path")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let size_bytes = platform_payload.get("size_bytes").and_then(Value::as_u64);
            let ocr = platform_payload
                .get("ocr")
                .cloned()
                .unwrap_or_else(|| json!({"status": "unavailable"}));
            let ocr_status = ocr
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unavailable")
                .to_owned();
            let ocr_characters = ocr
                .get("characters")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let (Some(staged_path), Some(size_bytes)) = (staged_path, size_bytes) else {
                fail_storage_import_operation(
                    &features,
                    &events,
                    &command_envelope,
                    &operation_id_for_task,
                    "invalid_platform_response",
                );
                return;
            };

            let import_result = if let Some(parent_node_id) = universal_parent {
                importer
                    .import_universal_file(
                        crate::storage::UniversalStagedImportRequest {
                            parent_node_id,
                            staged_path: PathBuf::from(staged_path),
                            original_filename: display_name,
                            detected_mime: Some(mime_type),
                            expected_size: Some(size_bytes),
                            duplicate_policy: crate::storage::DuplicatePolicy::RenameNewEntry,
                            source_uri_fingerprint: Some(source_fingerprint),
                        },
                        cancellation,
                    )
                    .await
            } else {
                importer
                    .import_workspace_file(
                        crate::storage::WorkspaceStagedImportRequest {
                            chat_id: chat_id.expect("workspace import validated chat id"),
                            staged_path: PathBuf::from(staged_path),
                            original_filename: display_name,
                            detected_mime: Some(mime_type),
                            expected_size: Some(size_bytes),
                            duplicate_policy: crate::storage::DuplicatePolicy::RenameNewEntry,
                            source_uri_fingerprint: Some(source_fingerprint),
                        },
                        cancellation,
                    )
                    .await
            };
            match import_result {
                Ok(receipt) => {
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Completed,
                        Some(1.0),
                        None,
                        json!({
                            "transaction_id": receipt.transaction_id.to_string(),
                            "node_id": receipt.node_id.to_string(),
                            "object_id": receipt.object_id.to_string(),
                            "display_name": receipt.display_name,
                            "size_bytes": receipt.plaintext_size,
                            "deduplicated": receipt.deduplicated,
                            "ocr": ocr,
                        }),
                    );
                    events.emit(
                        "application:storage",
                        "storage.file_imported",
                        json!({
                            "transaction_id": receipt.transaction_id.to_string(),
                            "node_id": receipt.node_id.to_string(),
                            "object_id": receipt.object_id.to_string(),
                            "display_name": receipt.display_name,
                            "size_bytes": receipt.plaintext_size,
                            "deduplicated": receipt.deduplicated,
                            "ocr_status": ocr_status,
                            "ocr_characters": ocr_characters,
                        }),
                        Some(&command_envelope),
                        Some(operation_id_for_task.clone()),
                    );
                    events.emit(
                        &format!("operation:{operation_id_for_task}"),
                        "operation.completed",
                        json!({"state": "completed"}),
                        Some(&command_envelope),
                        Some(operation_id_for_task),
                    );
                }
                Err(error) => fail_storage_import_operation(
                    &features,
                    &events,
                    &command_envelope,
                    &operation_id_for_task,
                    error.code(),
                ),
            }
        });
        acknowledgement
    }
}

#[cfg(not(feature = "rusqlite"))]
impl MukeiRuntime {
    fn import_storage_file(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        CommandAcknowledgementV2::rejected(
            Some(&command.envelope),
            RejectionReason::CapabilityUnavailable,
        )
    }
}

#[cfg(feature = "rusqlite")]
fn fail_storage_import_operation(
    features: &FeatureState,
    events: &EventBus,
    command: &CommandEnvelopeV2,
    operation_id: &str,
    code: &str,
) {
    features.update_operation(
        operation_id,
        OperationStatus::Failed,
        None,
        Some(code.to_string()),
        Value::Null,
    );
    events.emit(
        &format!("operation:{operation_id}"),
        "operation.failed",
        json!({"code": code}),
        Some(command),
        Some(operation_id.to_string()),
    );
}
