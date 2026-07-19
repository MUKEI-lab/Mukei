impl MukeiRuntime {
    /// Begin one process-local Temporary Chat session.
    ///
    /// IDs are minted by the runtime rather than supplied by the caller so an
    /// ephemeral session cannot intentionally reuse a durable conversation key.
    /// This API is intentionally not exposed through Protocol/JNI yet.
    pub fn begin_temporary_chat(&self) -> Option<(String, String)> {
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
    pub fn end_temporary_chat(&self, conversation: &str, branch: &str) -> bool {
        if Uuid::parse_str(conversation).is_err() || Uuid::parse_str(branch).is_err() {
            return false;
        }
        if !self.ephemeral_chats.end(conversation, branch) {
            return false;
        }
        self.purge_replay_for_conversation(conversation);
        self.events.emit(
            &format!("conversation:{conversation}"),
            "chat.temporary.ended",
            json!({"branch_id": branch}),
            None,
            None,
        );
        true
    }

    pub fn temporary_chat_active(&self, conversation: &str, branch: &str) -> bool {
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
        self.ephemeral_chats
            .history_if_registered(conversation, branch)
            .unwrap_or_else(|| self.features.history(conversation, branch))
    }

    fn append_chat_message(
        &self,
        conversation: &str,
        branch: &str,
        message: ChatMessage,
    ) -> bool {
        if self.ephemeral_chats.is_registered(conversation, branch) {
            self.ephemeral_chats
                .append_message(conversation, branch, message)
        } else {
            self.features.append_message(conversation, branch, message);
            true
        }
    }

    fn clear_chat_conversation(&self, conversation: &str, branch: &str) -> usize {
        if self.ephemeral_chats.is_registered(conversation, branch) {
            self.ephemeral_chats
                .clear_conversation(conversation, branch)
                .unwrap_or(0)
        } else {
            self.features.clear_conversation(conversation, branch)
        }
    }

    fn last_user_chat_message(&self, conversation: &str, branch: &str) -> Option<ChatMessage> {
        if self.ephemeral_chats.is_registered(conversation, branch) {
            self.ephemeral_chats.last_user_message(conversation, branch)
        } else {
            self.features.last_user_message(conversation, branch)
        }
    }

    fn remove_last_assistant_chat_message(&self, conversation: &str, branch: &str) -> bool {
        if self.ephemeral_chats.is_registered(conversation, branch) {
            self.ephemeral_chats
                .remove_last_assistant(conversation, branch)
        } else {
            self.features.remove_last_assistant(conversation, branch)
        }
    }

    fn accept_chat_operation(
        &self,
        command: &ValidatedCommand,
        conversation: &str,
        branch: &str,
    ) -> (CommandAcknowledgementV2, String, CancellationToken, bool) {
        let temporary = self.ephemeral_chats.is_registered(conversation, branch);
        if !temporary {
            let (acknowledgement, operation_id, token) = self.accept_operation(command);
            return (acknowledgement, operation_id, token, false);
        }

        let (operation_id, token) = self.ephemeral_chats.create_operation(
            conversation,
            branch,
            command.envelope.operation_id.as_deref(),
        );
        self.events.emit(
            &format!("operation:{operation_id}"),
            "operation.accepted",
            json!({"state": "accepted", "temporary": true}),
            Some(&command.envelope),
            Some(operation_id.clone()),
        );
        (
            CommandAcknowledgementV2::accepted(&command.envelope, Some(operation_id.clone())),
            operation_id,
            token,
            true,
        )
    }
}
