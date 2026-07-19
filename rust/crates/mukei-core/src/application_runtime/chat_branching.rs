fn clone_branch_prefix(
    source: &[ChatMessage],
    through_message_id: MessageId,
    new_branch: BranchId,
    replacement: Option<&str>,
) -> Result<Vec<ChatMessage>, RejectionReason> {
    let through_index = source
        .iter()
        .position(|message| message.id == through_message_id)
        .ok_or(RejectionReason::StaleScope)?;
    let target_role = source[through_index].role;
    if !matches!(target_role, Role::User | Role::Assistant) {
        return Err(RejectionReason::PolicyDenied);
    }

    let mut cloned = Vec::with_capacity(through_index + 1);
    let mut previous_id = None;
    for (index, source_message) in source.iter().take(through_index + 1).enumerate() {
        let is_target = index == through_index;
        let id = MessageId::new();
        cloned.push(ChatMessage {
            id,
            role: source_message.role,
            branch: new_branch,
            is_active: true,
            created_at: if is_target && replacement.is_some() {
                Utc::now()
            } else {
                source_message.created_at
            },
            content: if is_target {
                replacement
                    .map(str::to_owned)
                    .unwrap_or_else(|| source_message.content.clone())
            } else {
                source_message.content.clone()
            },
            parent: previous_id,
            token_count: if is_target && replacement.is_some() {
                None
            } else {
                source_message.token_count
            },
        });
        previous_id = Some(id);
    }
    Ok(cloned)
}

impl FeatureState {
    fn conversations_snapshot(&self) -> Value {
        let mut branches = self
            .conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(
                |((conversation_id, branch_id), messages)| ConversationProjection {
                    conversation_id: conversation_id.clone(),
                    branch_id: branch_id.clone(),
                    messages: messages.clone(),
                },
            )
            .collect::<Vec<_>>();
        branches.sort_by(|left, right| {
            left.conversation_id
                .cmp(&right.conversation_id)
                .then_with(|| left.branch_id.cmp(&right.branch_id))
        });
        json!({ "branches": branches })
    }

    fn fork_branch_through(
        &self,
        conversation: &str,
        source_branch: &str,
        through_message_id: MessageId,
        replacement: Option<&str>,
    ) -> Result<(String, ChatMessage), RejectionReason> {
        let new_branch_id = BranchId::new();
        let new_branch = new_branch_id.0.to_string();
        let cloned = {
            let mut conversations = self
                .conversations
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let source = conversations
                .get(&(conversation.to_owned(), source_branch.to_owned()))
                .ok_or(RejectionReason::StaleScope)?;
            let cloned = clone_branch_prefix(
                source,
                through_message_id,
                new_branch_id,
                replacement,
            )?;
            conversations.insert((conversation.to_owned(), new_branch.clone()), cloned.clone());
            cloned
        };
        self.persist_conversations();
        let target = cloned
            .last()
            .cloned()
            .ok_or(RejectionReason::StaleScope)?;
        Ok((new_branch, target))
    }
}

impl MukeiRuntime {
    fn command_on_branch(
        command: &ValidatedCommand,
        branch_id: &str,
    ) -> Result<ValidatedCommand, CommandAcknowledgementV2> {
        let mut forked = command.clone();
        let scope = forked.envelope.scope.as_mut().ok_or_else(|| {
            CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            )
        })?;
        scope.branch_id = Some(branch_id.to_owned());
        scope.turn_id = None;
        Ok(forked)
    }

    fn ensure_inference_ready_for_branching(
        &self,
        command: &ValidatedCommand,
    ) -> Result<(), CommandAcknowledgementV2> {
        if !self.activation.readiness_snapshot().active_backend_ready
            || self
                .agent_loop
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_none()
        {
            return Err(CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::BackendUnavailable,
            ));
        }
        Ok(())
    }

    fn edit_chat_message(
        &self,
        command: &ValidatedCommand,
        message_id: &str,
        replacement: &str,
    ) -> CommandAcknowledgementV2 {
        let (conversation, source_branch, _, _) = match Self::parse_chat_scope(command) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        let message_id = match Uuid::parse_str(message_id) {
            Ok(value) => MessageId(value),
            Err(_) => {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::InvalidPayload,
                )
            }
        };
        let source_message = self
            .features
            .conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(conversation.clone(), source_branch.clone()))
            .and_then(|messages| messages.iter().find(|message| message.id == message_id))
            .cloned();
        let Some(source_message) = source_message else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            );
        };
        if source_message.role == Role::User {
            if let Err(acknowledgement) = self.ensure_inference_ready_for_branching(command) {
                return acknowledgement;
            }
        } else if source_message.role != Role::Assistant {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::PolicyDenied,
            );
        }

        let (new_branch, edited_message) = match self.features.fork_branch_through(
            &conversation,
            &source_branch,
            message_id,
            Some(replacement.trim()),
        ) {
            Ok(value) => value,
            Err(reason) => {
                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
            }
        };
        let forked_command = match Self::command_on_branch(command, &new_branch) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        let source_message_id = message_id.0.to_string();

        if edited_message.role == Role::User {
            let acknowledgement = self.start_chat_operation(
                &forked_command,
                edited_message.content.clone(),
                false,
                Some(edited_message.clone()),
            );
            self.events.emit(
                &format!("conversation:{conversation}"),
                "chat.branch.forked",
                json!({
                    "conversation_id": conversation,
                    "source_branch_id": source_branch,
                    "new_branch_id": new_branch,
                    "source_message_id": source_message_id,
                    "edited_message_id": edited_message.id.0.to_string(),
                    "reason": "edit",
                }),
                Some(&forked_command.envelope),
                acknowledgement.operation_id.clone(),
            );
            acknowledgement
        } else {
            let (acknowledgement, operation_id, _) = self.accept_operation(&forked_command);
            self.features.update_operation(
                &operation_id,
                OperationStatus::Completed,
                Some(1.0),
                None,
                json!({
                    "conversation_id": conversation,
                    "source_branch_id": source_branch,
                    "new_branch_id": new_branch,
                    "source_message_id": source_message_id,
                    "edited_message_id": edited_message.id.0.to_string(),
                }),
            );
            self.events.emit(
                &format!("conversation:{conversation}"),
                "chat.branch.forked",
                json!({
                    "conversation_id": conversation,
                    "source_branch_id": source_branch,
                    "new_branch_id": new_branch,
                    "source_message_id": source_message_id,
                    "edited_message_id": edited_message.id.0.to_string(),
                    "reason": "edit",
                }),
                Some(&forked_command.envelope),
                Some(operation_id.clone()),
            );
            self.events.emit(
                &format!("operation:{operation_id}"),
                "operation.completed",
                json!({"state": "completed"}),
                Some(&forked_command.envelope),
                Some(operation_id),
            );
            acknowledgement
        }
    }

    fn regenerate_chat_branch(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(acknowledgement) = self.ensure_ready(command) {
            return acknowledgement;
        }
        if let Err(acknowledgement) = self.ensure_inference_ready_for_branching(command) {
            return acknowledgement;
        }
        let (conversation, source_branch, _, _) = match Self::parse_chat_scope(command) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        let Some(user_message) = self.features.last_user_message(&conversation, &source_branch) else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            );
        };
        let (new_branch, forked_user) = match self.features.fork_branch_through(
            &conversation,
            &source_branch,
            user_message.id,
            None,
        ) {
            Ok(value) => value,
            Err(reason) => {
                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
            }
        };
        let forked_command = match Self::command_on_branch(command, &new_branch) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        let acknowledgement = self.start_chat_operation(
            &forked_command,
            forked_user.content.clone(),
            true,
            Some(forked_user.clone()),
        );
        self.events.emit(
            &format!("conversation:{conversation}"),
            "chat.branch.forked",
            json!({
                "conversation_id": conversation,
                "source_branch_id": source_branch,
                "new_branch_id": new_branch,
                "source_message_id": user_message.id.0.to_string(),
                "edited_message_id": forked_user.id.0.to_string(),
                "reason": "regenerate",
            }),
            Some(&forked_command.envelope),
            acknowledgement.operation_id.clone(),
        );
        acknowledgement
    }
}

#[cfg(test)]
mod chat_branching_tests {
    use super::*;

    fn message(
        role: Role,
        branch: BranchId,
        content: &str,
        parent: Option<MessageId>,
    ) -> ChatMessage {
        ChatMessage {
            id: MessageId::new(),
            role,
            branch,
            is_active: true,
            created_at: Utc::now(),
            content: content.to_owned(),
            parent,
            token_count: None,
        }
    }

    #[test]
    fn edit_fork_rekeys_messages_and_preserves_source() {
        let source_branch = BranchId::new();
        let first = message(Role::User, source_branch, "one", None);
        let second = message(Role::Assistant, source_branch, "two", Some(first.id));
        let source = vec![first.clone(), second];
        let new_branch = BranchId::new();

        let fork = clone_branch_prefix(&source, first.id, new_branch, Some("edited"))
            .expect("fork");

        assert_eq!(source[0].content, "one");
        assert_eq!(fork.len(), 1);
        assert_eq!(fork[0].content, "edited");
        assert_ne!(fork[0].id, first.id);
        assert_eq!(fork[0].branch, new_branch);
        assert!(fork[0].parent.is_none());
    }

    #[test]
    fn regenerate_fork_builds_fresh_linear_parent_chain() {
        let source_branch = BranchId::new();
        let first = message(Role::User, source_branch, "one", None);
        let second = message(Role::Assistant, source_branch, "two", Some(first.id));
        let third = message(Role::User, source_branch, "three", None);
        let source = vec![first, second, third.clone()];
        let new_branch = BranchId::new();

        let fork = clone_branch_prefix(&source, third.id, new_branch, None).expect("fork");

        assert_eq!(fork.len(), 3);
        assert!(fork.iter().all(|message| message.branch == new_branch));
        assert_ne!(fork[2].id, third.id);
        assert_eq!(fork[1].parent, Some(fork[0].id));
        assert_eq!(fork[2].parent, Some(fork[1].id));
    }

    #[test]
    fn tool_messages_cannot_be_direct_edit_targets() {
        let branch = BranchId::new();
        let tool = message(Role::Tool, branch, "tool", None);
        let result = clone_branch_prefix(&[tool.clone()], tool.id, BranchId::new(), Some("x"));
        assert_eq!(result.unwrap_err(), RejectionReason::PolicyDenied);
    }
}
