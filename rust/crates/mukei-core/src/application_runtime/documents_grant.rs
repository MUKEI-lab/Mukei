impl MukeiRuntime {
    fn grant_document(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(ack) = self.ensure_ready(command) {
            return ack;
        }
        let ValidatedCommandPayload::DocumentGrant(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        let (acknowledgement, operation_id, token) = self.accept_operation(command);
        let document_id = Uuid::new_v4().to_string();
        self.features.insert_document(DocumentProjection {
            document_id: document_id.clone(),
            label: payload.label.clone(),
            mime_type: payload.mime_type.clone(),
            source_fingerprint: blake3::hash(payload.target.as_bytes()).to_hex().to_string(),
            staged_path: None,
            size_bytes: None,
            status: DocumentStatus::Staging,
            error_code: None,
        });
        let request_id = match self.platform.enqueue(
            Some(operation_id.clone()),
            PlatformRequestKind::DocumentStage {
                target: payload.target.clone(),
                label: payload.label.clone(),
                mime_type: payload.mime_type.clone(),
            },
        ) {
            Ok(value) => value,
            Err(_) => {
                self.features.update_document(&document_id, |document| {
                    document.status = DocumentStatus::Failed;
                    document.error_code = Some("platform_queue_full".into());
                });
                self.features.update_operation(
                    &operation_id,
                    OperationStatus::Failed,
                    None,
                    Some("platform_queue_full".into()),
                    Value::Null,
                );
                return acknowledgement;
            }
        };
        let platform = Arc::clone(&self.platform);
        let features = Arc::clone(&self.features);
        let events = Arc::clone(&self.events);
        let command_envelope = command.envelope.clone();
        let operation_id_for_task = operation_id.clone();
        self.async_runtime.handle().spawn(async move {
            match platform
                .wait_for_response(&request_id, PLATFORM_WAIT_TIMEOUT, token)
                .await
            {
                Ok(payload) => {
                    let staged_path = payload
                        .get("staged_path")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let size_bytes = payload.get("size_bytes").and_then(Value::as_u64);
                    let ocr = payload
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
                    let Some(staged_path_value) = staged_path.clone() else {
                        features.update_document(&document_id, |document| {
                            document.status = DocumentStatus::Failed;
                            document.error_code = Some("invalid_platform_response".into());
                        });
                        features.update_operation(
                            &operation_id_for_task,
                            OperationStatus::Failed,
                            None,
                            Some("invalid_platform_response".into()),
                            Value::Null,
                        );
                        return;
                    };
                    features.update_document(&document_id, |document| {
                        document.staged_path = Some(staged_path_value);
                        document.size_bytes = size_bytes;
                        document.status = DocumentStatus::Staged;
                        document.error_code = None;
                    });
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Completed,
                        Some(1.0),
                        None,
                        json!({
                            "document_id": document_id,
                            "staged_path": staged_path,
                            "size_bytes": size_bytes,
                            "ocr": ocr,
                        }),
                    );
                    events.emit(
                        "application:documents",
                        "document.granted",
                        json!({
                            "document_id": document_id,
                            "size_bytes": size_bytes,
                            "ocr_status": ocr_status,
                            "ocr_characters": ocr_characters,
                        }),
                        Some(&command_envelope),
                        Some(operation_id_for_task.clone()),
                    );
                    events.emit(
                        &format!("operation:{}", operation_id_for_task),
                        "operation.completed",
                        json!({"state": "completed"}),
                        Some(&command_envelope),
                        Some(operation_id_for_task),
                    );
                }
                Err(error) => {
                    features.update_document(&document_id, |document| {
                        document.status = DocumentStatus::Failed;
                        document.error_code = Some(error.to_string());
                    });
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Failed,
                        None,
                        Some(error.to_string()),
                        Value::Null,
                    );
                    events.emit(
                        &format!("operation:{}", operation_id_for_task),
                        "operation.failed",
                        json!({"code": "platform_document_stage_failed"}),
                        Some(&command_envelope),
                        Some(operation_id_for_task),
                    );
                }
            }
        });
        acknowledgement
    }
}
