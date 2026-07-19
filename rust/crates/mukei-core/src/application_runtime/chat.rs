impl MukeiRuntime {
    fn parse_chat_scope(
        command: &ValidatedCommand,
    ) -> Result<(String, String, ConversationId, BranchId), CommandAcknowledgementV2> {
        let scope = command.envelope.scope.as_ref().ok_or_else(|| {
            CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            )
        })?;
        let conversation = scope.conversation_id.as_deref().ok_or_else(|| {
            CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            )
        })?;
        let branch = scope.branch_id.as_deref().ok_or_else(|| {
            CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            )
        })?;
        let conversation_id = Uuid::parse_str(conversation)
            .map(ConversationId)
            .map_err(|_| {
                CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::StaleScope,
                )
            })?;
        let branch_id = Uuid::parse_str(branch).map(BranchId).map_err(|_| {
            CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            )
        })?;
        Ok((
            conversation.to_owned(),
            branch.to_owned(),
            conversation_id,
            branch_id,
        ))
    }

    fn send_message(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(ack) = self.ensure_ready(command) {
            return ack;
        }
        let ValidatedCommandPayload::SendMessage(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        if let Some(message_id) = command
            .envelope
            .scope
            .as_ref()
            .and_then(|scope| scope.turn_id.as_deref())
        {
            if payload.project_id.is_some() {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::InvalidPayload,
                );
            }
            return self.edit_chat_message(command, message_id, &payload.text);
        }
        if let Some(project_id) = payload.project_id.as_deref() {
            if let Err(acknowledgement) = self.ensure_inference_ready_for_branching(command) {
                return acknowledgement;
            }
            let (conversation, _, _, _) = match Self::parse_chat_scope(command) {
                Ok(value) => value,
                Err(acknowledgement) => return acknowledgement,
            };
            if let Err(reason) = self
                .features
                .bind_conversation_project(&conversation, project_id)
            {
                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason);
            }
        }
        self.start_chat_operation(command, payload.text.clone(), false, None)
    }

    fn start_chat_operation(
        &self,
        command: &ValidatedCommand,
        text: String,
        regenerate: bool,
        existing_user: Option<ChatMessage>,
    ) -> CommandAcknowledgementV2 {
        let (conversation, branch, conversation_id, branch_id) =
            match Self::parse_chat_scope(command) {
                Ok(value) => value,
                Err(ack) => return ack,
            };
        if !self.activation.readiness_snapshot().active_backend_ready {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::BackendUnavailable,
            );
        }
        let Some(agent_loop) = self
            .agent_loop
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::BackendUnavailable,
            );
        };

        let project_context = match self
            .features
            .project_context_message(&conversation, branch_id)
        {
            Ok(value) => value,
            Err(reason) => {
                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
            }
        };

        let user_message = existing_user.unwrap_or_else(|| {
            let mut message = ChatMessage::user_with_id(MessageId::new(), branch_id, text.clone());
            message.parent = self
                .features
                .history(conversation_id, branch_id)
                .last()
                .map(|value| value.id);
            message
        });
        let user_message_id = user_message.id;
        if self
            .features
            .history(conversation_id, branch_id)
            .iter()
            .all(|message| message.id != user_message_id)
        {
            self.features
                .append_message(&conversation, &branch, user_message.clone());
        }
        let mut seed_history = Vec::with_capacity(2);
        if let Some(project_context) = project_context {
            seed_history.push(project_context);
        }
        seed_history.push(user_message.clone());
        let (acknowledgement, operation_id, operation_token) = self.accept_operation(command);
        let events = Arc::clone(&self.events);
        let features = Arc::clone(&self.features);
        let command_envelope = command.envelope.clone();
        let runtime_cancel = self.cancellation.child_token();
        let child_cancel = operation_token.child_token();
        let operation_id_for_task = operation_id.clone();
        self.async_runtime.handle().spawn(async move {
            features.update_operation(
                &operation_id_for_task,
                OperationStatus::Running,
                None,
                None,
                Value::Null,
            );
            events.emit(
                &format!("conversation:{conversation}"),
                if regenerate {
                    "chat.regeneration.started"
                } else {
                    "chat.generation.started"
                },
                json!({"user_message_id": user_message_id.0.to_string()}),
                Some(&command_envelope),
                Some(operation_id_for_task.clone()),
            );
            let (token_sender, mut token_receiver) = mpsc::channel(64);
            let cancellation = CancellationToken::new();
            let cancellation_for_parent = cancellation.clone();
            let combined_cancel = cancellation.clone();
            let watcher_done = CancellationToken::new();
            let watcher_done_task = watcher_done.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = runtime_cancel.cancelled() => cancellation_for_parent.cancel(),
                    _ = child_cancel.cancelled() => cancellation_for_parent.cancel(),
                    _ = watcher_done_task.cancelled() => {},
                }
            });
            let run = agent_loop.run_seeded(
                seed_history,
                conversation_id,
                branch_id,
                combined_cancel,
                token_sender,
                None,
            );
            tokio::pin!(run);
            let outcome = loop {
                tokio::select! {
                    result = &mut run => break result,
                    token = token_receiver.recv() => {
                        if let Some(token) = token {
                            events.emit(
                                &format!("operation:{}", operation_id_for_task),
                                "chat.token.delta",
                                json!({"text": token}),
                                Some(&command_envelope),
                                Some(operation_id_for_task.clone()),
                            );
                        }
                    }
                }
            };
            watcher_done.cancel();
            while let Ok(token) = token_receiver.try_recv() {
                events.emit(
                    &format!("operation:{}", operation_id_for_task),
                    "chat.token.delta",
                    json!({"text": token}),
                    Some(&command_envelope),
                    Some(operation_id_for_task.clone()),
                );
            }
            match outcome {
                Ok(outcome) if outcome.cancelled => {
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Cancelled,
                        None,
                        Some("cancelled".into()),
                        Value::Null,
                    );
                    events.emit(
                        &format!("operation:{}", operation_id_for_task),
                        "operation.cancelled",
                        json!({"state": "cancelled"}),
                        Some(&command_envelope),
                        Some(operation_id_for_task),
                    );
                }
                Ok(outcome) => {
                    if let Some(content) = outcome.final_content.clone() {
                        features.append_message(
                            &conversation,
                            &branch,
                            ChatMessage {
                                id: MessageId::new(),
                                role: Role::Assistant,
                                branch: branch_id,
                                is_active: true,
                                created_at: Utc::now(),
                                content: content.clone(),
                                parent: Some(outcome.final_parent),
                                token_count: outcome.final_token_count,
                            },
                        );
                    }
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Completed,
                        Some(1.0),
                        None,
                        json!({
                            "content": outcome.final_content,
                            "token_count": outcome.final_token_count,
                        }),
                    );
                    events.emit(
                        &format!("conversation:{conversation}"),
                        "chat.generation.completed",
                        json!({
                            "content": outcome.final_content,
                            "token_count": outcome.final_token_count,
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
                }
            }
        });
        acknowledgement
    }

    fn stop_generation(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        let Some(operation_id) = command.envelope.operation_id.as_deref() else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            );
        };
        if !self.features.cancel_operation(operation_id) {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            );
        }
        self.events.emit(
            &format!("operation:{operation_id}"),
            "operation.cancel_requested",
            json!({"state": "cancel_requested"}),
            Some(&command.envelope),
            Some(operation_id.to_owned()),
        );
        CommandAcknowledgementV2::accepted(&command.envelope, Some(operation_id.to_owned()))
    }

    fn clear_conversation(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(ack) = self.ensure_ready(command) {
            return ack;
        }
        let (conversation, branch, _, _) = match Self::parse_chat_scope(command) {
            Ok(value) => value,
            Err(ack) => return ack,
        };
        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        let removed = self.features.clear_conversation(&conversation, &branch);
        self.features.update_operation(
            &operation_id,
            OperationStatus::Completed,
            Some(1.0),
            None,
            json!({"removed_messages": removed}),
        );
        self.events.emit(
            &format!("conversation:{conversation}"),
            "chat.conversation.cleared",
            json!({"branch_id": branch, "removed_messages": removed}),
            Some(&command.envelope),
            Some(operation_id.clone()),
        );
        acknowledgement
    }
}
