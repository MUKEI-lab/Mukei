impl MukeiRuntime {
    /// Begin one process-local Temporary Chat session.
    ///
    /// IDs are minted by the runtime rather than supplied by the caller so an
    /// ephemeral session cannot intentionally reuse a durable conversation key.
    /// This remains crate-private until RAG/session isolation and Protocol/JNI
    /// exposure are implemented and tested.
    pub(crate) fn begin_temporary_chat(&self) -> Option<(String, String)> {
        if self.closed.load(Ordering::Acquire) || self.state() != RuntimeState::Ready {
            return None;
        }
        for _ in 0..8 {
            let conversation_id = ConversationId::new();
            let branch_id = BranchId::new();
            if !self.features.history(conversation_id, branch_id).is_empty() {
                continue;
            }
            let conversation = conversation_id.0.to_string();
            let branch = branch_id.0.to_string();
            if !self.ephemeral_chats.begin(&conversation, &branch) {
                continue;
            }
            self.events.emit(
                &format!("conversation:{conversation}"),
                "chat.temporary.started",
                json!({"branch_id": branch}),
                None,
                None,
            );
            return Some((conversation, branch));
        }
        None
    }

    /// End and purge one Temporary Chat session.
    pub(crate) fn end_temporary_chat(&self, conversation: &str, branch: &str) -> bool {
        if Uuid::parse_str(conversation).is_err() || Uuid::parse_str(branch).is_err() {
            return false;
        }
        let operation_ids = match self.ephemeral_chats.end(conversation, branch) {
            Some(operation_ids) => operation_ids,
            None => return false,
        };
        self.purge_replay_for_conversation(conversation);
        let purged_events = self
            .events
            .purge_temporary_chat(conversation, &operation_ids);
        // The conversation stream is now intentionally suppressed. Publish the
        // non-sensitive lifecycle signal on an application stream instead.
        self.events.emit(
            "application:temporary-chat",
            "chat.temporary.ended",
            json!({
                "conversation_id": conversation,
                "branch_id": branch,
                "purged_events": purged_events,
            }),
            None,
            None,
        );
        true
    }

    pub(crate) fn temporary_chat_active(&self, conversation: &str, branch: &str) -> bool {
        self.ephemeral_chats.is_registered(conversation, branch)
    }

    fn purge_replay_for_conversation(&self, conversation: &str) {
        self.replay
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|_, record| {
                let belongs_to_conversation = serde_json::from_slice::<Value>(&record.fingerprint)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("scope")
                            .and_then(|scope| scope.get("conversation_id"))
                            .and_then(Value::as_str)
                            .map(|value| value == conversation)
                    })
                    .unwrap_or(false);
                !belongs_to_conversation
            });
    }

    fn chat_history(&self, conversation: ConversationId, branch: BranchId) -> Vec<ChatMessage> {
        let conversation_key = conversation.0.to_string();
        let branch_key = branch.0.to_string();
        match self
            .ephemeral_chats
            .session_state(&conversation_key, &branch_key)
        {
            EphemeralSessionState::Active => self
                .ephemeral_chats
                .history_if_registered(conversation, branch)
                .unwrap_or_default(),
            EphemeralSessionState::Retired => Vec::new(),
            EphemeralSessionState::Absent => self.features.history(conversation, branch),
        }
    }

    fn append_chat_message(
        &self,
        conversation: &str,
        branch: &str,
        message: ChatMessage,
    ) -> bool {
        match self.ephemeral_chats.session_state(conversation, branch) {
            EphemeralSessionState::Active => self
                .ephemeral_chats
                .append_message(conversation, branch, message),
            EphemeralSessionState::Retired => false,
            EphemeralSessionState::Absent => {
                self.features.append_message(conversation, branch, message);
                true
            }
        }
    }

    fn clear_chat_conversation(&self, conversation: &str, branch: &str) -> usize {
        match self.ephemeral_chats.session_state(conversation, branch) {
            EphemeralSessionState::Active => self
                .ephemeral_chats
                .clear_conversation(conversation, branch)
                .unwrap_or(0),
            EphemeralSessionState::Retired => 0,
            EphemeralSessionState::Absent => {
                self.features.clear_conversation(conversation, branch)
            }
        }
    }

    fn last_user_chat_message(&self, conversation: &str, branch: &str) -> Option<ChatMessage> {
        match self.ephemeral_chats.session_state(conversation, branch) {
            EphemeralSessionState::Active => {
                self.ephemeral_chats.last_user_message(conversation, branch)
            }
            EphemeralSessionState::Retired => None,
            EphemeralSessionState::Absent => {
                self.features.last_user_message(conversation, branch)
            }
        }
    }

    fn remove_last_assistant_chat_message(&self, conversation: &str, branch: &str) -> bool {
        match self.ephemeral_chats.session_state(conversation, branch) {
            EphemeralSessionState::Active => self
                .ephemeral_chats
                .remove_last_assistant(conversation, branch),
            EphemeralSessionState::Retired => false,
            EphemeralSessionState::Absent => {
                self.features.remove_last_assistant(conversation, branch)
            }
        }
    }

    fn accept_chat_operation(
        &self,
        command: &ValidatedCommand,
        conversation: &str,
        branch: &str,
    ) -> Result<
        (CommandAcknowledgementV2, String, CancellationToken, bool),
        CommandAcknowledgementV2,
    > {
        match self.ephemeral_chats.session_state(conversation, branch) {
            EphemeralSessionState::Retired => Err(CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            )),
            EphemeralSessionState::Absent => {
                let (acknowledgement, operation_id, token) = self.accept_operation(command);
                Ok((acknowledgement, operation_id, token, false))
            }
            EphemeralSessionState::Active => {
                let Some((operation_id, token)) = self.ephemeral_chats.create_operation(
                    conversation,
                    branch,
                    command.envelope.operation_id.as_deref(),
                ) else {
                    return Err(CommandAcknowledgementV2::rejected(
                        Some(&command.envelope),
                        RejectionReason::StaleScope,
                    ));
                };
                self.events.emit(
                    &format!("operation:{operation_id}"),
                    "operation.accepted",
                    json!({"state": "accepted", "temporary": true}),
                    Some(&command.envelope),
                    Some(operation_id.clone()),
                );
                Ok((
                    CommandAcknowledgementV2::accepted(
                        &command.envelope,
                        Some(operation_id.clone()),
                    ),
                    operation_id,
                    token,
                    true,
                ))
            }
        }
    }
}
