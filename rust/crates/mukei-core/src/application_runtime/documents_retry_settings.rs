impl MukeiRuntime {
    fn retry_document_ingestion(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(ack) = self.ensure_ready(command) { return ack; }
        let ValidatedCommandPayload::Document(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), RejectionReason::InvalidPayload);
        };
        let exists = self.features.documents.read().unwrap_or_else(|p| p.into_inner()).get(&payload.document_id).is_some_and(|document| document.staged_path.is_some());
        if !exists {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), RejectionReason::StaleScope);
        }
        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        if let Some(document) = self.features.documents.write().unwrap_or_else(|p| p.into_inner()).get_mut(&payload.document_id) {
            document.status = DocumentStatus::IngestionUnavailable;
            document.error_code = Some("rag_ingestion_backend_unavailable".into());
        }
        self.features.update_operation(&operation_id, OperationStatus::Failed, None, Some("rag_ingestion_backend_unavailable".into()), Value::Null);
        self.events.emit(&format!("operation:{operation_id}"), "operation.failed", json!({"code": "rag_ingestion_backend_unavailable", "document_id": payload.document_id}), Some(&command.envelope), Some(operation_id));
        acknowledgement
    }

    fn update_setting(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(ack) = self.ensure_ready(command) { return ack; }
        let ValidatedCommandPayload::SettingUpdate(setting) = &command.payload else {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), RejectionReason::InvalidPayload);
        };
        self.settings.write().unwrap_or_else(|p| p.into_inner()).insert(setting.key.clone(), setting.value.clone());
        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        self.features.update_operation(&operation_id, OperationStatus::Completed, Some(1.0), None, json!({"key": setting.key, "value": setting.value}));
        self.events.emit("application:settings", "settings.updated", json!({"key": setting.key, "value": setting.value}), Some(&command.envelope), Some(operation_id));
        acknowledgement
    }

    fn recover_chat(&self, command: &ValidatedCommand, regenerate: bool) -> CommandAcknowledgementV2 {
        if let Err(ack) = self.ensure_ready(command) { return ack; }
        let (conversation, branch, _, _) = match Self::parse_chat_scope(command) {
            Ok(value) => value,
            Err(ack) => return ack,
        };
        let Some(user_message) = self.features.last_user_message(&conversation, &branch) else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            );
        };
        if regenerate {
            self.features.remove_last_assistant(&conversation, &branch);
        }
        self.start_chat_operation(
            command,
            user_message.content.clone(),
            regenerate,
            Some(user_message),
        )
    }

}
