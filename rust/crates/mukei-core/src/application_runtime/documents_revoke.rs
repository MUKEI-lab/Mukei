impl MukeiRuntime {
    fn revoke_document(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(ack) = self.ensure_ready(command) {
            return ack;
        }
        let ValidatedCommandPayload::Document(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        let Some(document) = self.features.document(&payload.document_id) else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            );
        };
        let (acknowledgement, operation_id, token) = self.accept_operation(command);
        let document_id = payload.document_id.clone();
        let rag_service = self
            .rag_service
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(staged_path) = document.staged_path else {
            self.features.remove_document(&document_id);
            self.features.update_operation(
                &operation_id,
                OperationStatus::Completed,
                Some(1.0),
                None,
                json!({"document_id": document_id}),
            );
            return acknowledgement;
        };
        let request_id = match self.platform.enqueue(
            Some(operation_id.clone()),
            PlatformRequestKind::DocumentDelete { staged_path },
        ) {
            Ok(request_id) => request_id,
            Err(error) => {
                self.features.update_operation(
                    &operation_id,
                    OperationStatus::Failed,
                    None,
                    Some(error.to_string()),
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
            let vector_cleanup = match rag_service {
                Some(service) => service.revoke_document(&document_id).await,
                None => Ok(0),
            };
            if let Err(error) = vector_cleanup {
                features.update_operation(
                    &operation_id_for_task,
                    OperationStatus::Failed,
                    None,
                    Some(error.error_code().into()),
                    Value::Null,
                );
                events.emit(
                    &format!("operation:{}", operation_id_for_task),
                    "operation.failed",
                    json!({"code": error.error_code()}),
                    Some(&command_envelope),
                    Some(operation_id_for_task),
                );
                return;
            }
            match platform
                .wait_for_response(&request_id, PLATFORM_WAIT_TIMEOUT, token)
                .await
            {
                Ok(_) => {
                    features.remove_document(&document_id);
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Completed,
                        Some(1.0),
                        None,
                        json!({"document_id": document_id}),
                    );
                    events.emit(
                        "application:documents",
                        "document.revoked",
                        json!({"document_id": document_id}),
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
                        json!({"code": "platform_document_delete_failed"}),
                        Some(&command_envelope),
                        Some(operation_id_for_task),
                    );
                }
            }
        });
        acknowledgement
    }
}
