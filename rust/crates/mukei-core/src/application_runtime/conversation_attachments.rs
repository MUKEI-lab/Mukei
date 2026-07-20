const MAX_ATTACHMENT_CONTEXT_BYTES_PER_FILE: usize = 16 * 1024;
const MAX_ATTACHMENT_CONTEXT_BYTES_TOTAL: usize = 48 * 1024;

#[cfg(feature = "rusqlite")]
impl MukeiRuntime {
    fn conversation_attachment_port(
        &self,
        command: &ValidatedCommand,
    ) -> Result<Arc<dyn crate::storage::ConversationAttachmentPort>, CommandAcknowledgementV2> {
        self.conversation_attachments
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(|| {
                CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::CapabilityUnavailable,
                )
            })
    }

    fn add_conversation_attachment(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        self.mutate_conversation_attachment(command, true)
    }

    fn remove_conversation_attachment(
        &self,
        command: &ValidatedCommand,
    ) -> CommandAcknowledgementV2 {
        self.mutate_conversation_attachment(command, false)
    }

    fn mutate_conversation_attachment(
        &self,
        command: &ValidatedCommand,
        add: bool,
    ) -> CommandAcknowledgementV2 {
        if let Err(acknowledgement) = self.ensure_ready(command) {
            return acknowledgement;
        }
        let ValidatedCommandPayload::ConversationAttachment(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        let conversation = match Self::parse_conversation_scope(command) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        if let Err(reason) = self.features.ensure_active_conversation(&conversation) {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason);
        }
        let service = match self.conversation_attachment_port(command) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        let Some(node_id) = Uuid::parse_str(&payload.node_id)
            .ok()
            .map(crate::storage::StorageNodeId)
        else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            );
        };

        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        let features = Arc::clone(&self.features);
        let events = Arc::clone(&self.events);
        let envelope = command.envelope.clone();
        let operation_id_for_task = operation_id.clone();
        self.async_runtime.handle().spawn(async move {
            if add {
                match service.add_attachment(conversation.clone(), node_id).await {
                    Ok(attachment) => complete_conversation_attachment_operation(
                        &features,
                        &events,
                        &envelope,
                        &operation_id_for_task,
                        &conversation,
                        "conversation.attachment.added",
                        serde_json::to_value(attachment).unwrap_or(Value::Null),
                    ),
                    Err(_) => fail_conversation_attachment_operation(
                        &features,
                        &events,
                        &envelope,
                        &operation_id_for_task,
                        "conversation_attachment_add_failed",
                    ),
                }
            } else {
                match service.remove_attachment(conversation.clone(), node_id).await {
                    Ok(true) => complete_conversation_attachment_operation(
                        &features,
                        &events,
                        &envelope,
                        &operation_id_for_task,
                        &conversation,
                        "conversation.attachment.removed",
                        json!({"conversation_id": conversation, "node_id": node_id.to_string()}),
                    ),
                    Ok(false) | Err(_) => fail_conversation_attachment_operation(
                        &features,
                        &events,
                        &envelope,
                        &operation_id_for_task,
                        "conversation_attachment_remove_failed",
                    ),
                }
            }
        });
        acknowledgement
    }

    fn attachment_context_messages(
        &self,
        conversation_id: &str,
        branch: BranchId,
    ) -> Result<Vec<ChatMessage>, RejectionReason> {
        let service = self
            .conversation_attachments
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(service) = service else {
            return Ok(Vec::new());
        };
        let contexts = self
            .async_runtime
            .block_on(service.load_context(
                conversation_id.to_owned(),
                MAX_ATTACHMENT_CONTEXT_BYTES_PER_FILE,
                MAX_ATTACHMENT_CONTEXT_BYTES_TOTAL,
            ))
            .map_err(|_| RejectionReason::PolicyDenied)?;

        Ok(contexts
            .into_iter()
            .map(|context| {
                let mut reference = format!("Attached file: {}\n", context.display_name);
                if let Some(mime_type) = context.mime_type.as_deref() {
                    reference.push_str("MIME type: ");
                    reference.push_str(mime_type);
                    reference.push('\n');
                }
                if context.truncated {
                    reference.push_str("Context note: file content was truncated to the bounded prompt budget.\n");
                }
                reference.push('\n');
                reference.push_str(&context.content);
                ChatMessage {
                    id: MessageId::new(),
                    role: Role::System,
                    branch,
                    is_active: true,
                    created_at: Utc::now(),
                    content: crate::tools::sentinel::wrap_external_data(
                        crate::tools::sentinel::ExternalDataSource::File,
                        &reference,
                    ),
                    parent: None,
                    token_count: None,
                }
            })
            .collect())
    }
}

#[cfg(feature = "rusqlite")]
fn complete_conversation_attachment_operation(
    features: &FeatureState,
    events: &EventBus,
    command: &CommandEnvelopeV2,
    operation_id: &str,
    conversation_id: &str,
    event_type: &str,
    result: Value,
) {
    features.update_operation(
        operation_id,
        OperationStatus::Completed,
        Some(1.0),
        None,
        result.clone(),
    );
    events.emit(
        &format!("conversation:{conversation_id}"),
        event_type,
        result,
        Some(command),
        Some(operation_id.to_owned()),
    );
    events.emit(
        &format!("operation:{operation_id}"),
        "operation.completed",
        json!({"state": "completed"}),
        Some(command),
        Some(operation_id.to_owned()),
    );
}

#[cfg(feature = "rusqlite")]
fn fail_conversation_attachment_operation(
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
        Some(code.to_owned()),
        Value::Null,
    );
    events.emit(
        &format!("operation:{operation_id}"),
        "operation.failed",
        json!({"code": code}),
        Some(command),
        Some(operation_id.to_owned()),
    );
}

#[cfg(not(feature = "rusqlite"))]
impl MukeiRuntime {
    fn add_conversation_attachment(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        CommandAcknowledgementV2::rejected(
            Some(&command.envelope),
            RejectionReason::CapabilityUnavailable,
        )
    }

    fn remove_conversation_attachment(
        &self,
        command: &ValidatedCommand,
    ) -> CommandAcknowledgementV2 {
        self.add_conversation_attachment(command)
    }

    fn attachment_context_messages(
        &self,
        _conversation_id: &str,
        _branch: BranchId,
    ) -> Result<Vec<ChatMessage>, RejectionReason> {
        Ok(Vec::new())
    }
}
