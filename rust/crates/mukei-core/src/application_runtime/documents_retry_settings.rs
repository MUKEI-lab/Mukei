impl MukeiRuntime {
    fn retry_document_ingestion(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
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
        let Some(staged_path) = document.staged_path.clone() else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            );
        };
        let rag_service = self
            .rag_service
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(rag_service) = rag_service else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::CapabilityUnavailable,
            );
        };

        let (acknowledgement, operation_id, token) = self.accept_operation(command);
        let features = Arc::clone(&self.features);
        let events = Arc::clone(&self.events);
        let command_envelope = command.envelope.clone();
        let document_id = payload.document_id.clone();
        let mime_type = document.mime_type.clone();
        let operation_id_for_task = operation_id.clone();
        self.async_runtime.handle().spawn(async move {
            features.update_operation(
                &operation_id_for_task,
                OperationStatus::Running,
                Some(0.0),
                None,
                Value::Null,
            );
            let outcome = tokio::select! {
                _ = token.cancelled() => Err(MukeiError::Cancelled),
                result = rag_service.ingest_document(
                    &document_id,
                    Path::new(&staged_path),
                    &mime_type,
                ) => result,
            };
            match outcome {
                Ok(result) => {
                    features.update_document(&document_id, |document| {
                        document.status = DocumentStatus::Indexed;
                        document.error_code = None;
                    });
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Completed,
                        Some(1.0),
                        None,
                        json!({
                            "document_id": document_id,
                            "chunk_count": result.chunk_count,
                        }),
                    );
                    events.emit(
                        "application:documents",
                        "document.indexed",
                        json!({
                            "document_id": document_id,
                            "chunk_count": result.chunk_count,
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
                    let status = if matches!(error, MukeiError::Cancelled) {
                        OperationStatus::Cancelled
                    } else {
                        OperationStatus::Failed
                    };
                    features.update_document(&document_id, |document| {
                        document.status = DocumentStatus::Failed;
                        document.error_code = Some(error.error_code().into());
                    });
                    features.update_operation(
                        &operation_id_for_task,
                        status,
                        None,
                        Some(error.error_code().into()),
                        Value::Null,
                    );
                    events.emit(
                        &format!("operation:{}", operation_id_for_task),
                        if matches!(status, OperationStatus::Cancelled) {
                            "operation.cancelled"
                        } else {
                            "operation.failed"
                        },
                        json!({"code": error.error_code(), "document_id": document_id}),
                        Some(&command_envelope),
                        Some(operation_id_for_task),
                    );
                }
            }
        });
        acknowledgement
    }

    fn update_setting(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(ack) = self.ensure_ready(command) {
            return ack;
        }
        let ValidatedCommandPayload::SettingUpdate(setting) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };

        let remote_policy = if setting.key == "remote_feature_policy" {
            let Some(value) = setting.value.as_str() else {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::InvalidPayload,
                );
            };
            match value.parse::<crate::tools::RemoteFeaturePolicy>() {
                Ok(policy) => Some(policy),
                Err(_) => {
                    return CommandAcknowledgementV2::rejected(
                        Some(&command.envelope),
                        RejectionReason::InvalidPayload,
                    )
                }
            }
        } else {
            None
        };

        let previous = self
            .settings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(setting.key.clone(), setting.value.clone());
        if self.persist_settings_now().is_err() {
            let mut settings = self
                .settings
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match previous {
                Some(value) => {
                    settings.insert(setting.key.clone(), value);
                }
                None => {
                    settings.remove(&setting.key);
                }
            }
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::BackendUnavailable,
            );
        }
        if let Some(policy) = remote_policy {
            self.set_remote_policy(policy);
        }

        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        self.features.update_operation(
            &operation_id,
            OperationStatus::Completed,
            Some(1.0),
            None,
            json!({"key": setting.key, "value": setting.value}),
        );
        self.events.emit(
            "application:settings",
            "settings.updated",
            json!({"key": setting.key, "value": setting.value}),
            Some(&command.envelope),
            Some(operation_id),
        );
        acknowledgement
    }

    fn recover_chat(&self, command: &ValidatedCommand, regenerate: bool) -> CommandAcknowledgementV2 {
        if let Err(ack) = self.ensure_ready(command) {
            return ack;
        }
        let (conversation, branch, _, _) = match Self::parse_chat_scope(command) {
            Ok(value) => value,
            Err(ack) => return ack,
        };
        let Some(user_message) = self.last_user_chat_message(&conversation, &branch) else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            );
        };
        if regenerate {
            self.remove_last_assistant_chat_message(&conversation, &branch);
        }
        self.start_chat_operation(
            command,
            user_message.content.clone(),
            regenerate,
            Some(user_message),
        )
    }
}
