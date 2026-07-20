const MAX_CONVERSATION_TITLE_CHARS: usize = 128;

fn conversation_title_from_text(text: &str) -> String {
    let normalized = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("New conversation");
    let title = normalized
        .chars()
        .take(MAX_CONVERSATION_TITLE_CHARS)
        .collect::<String>();
    if title.is_empty() {
        "New conversation".to_owned()
    } else {
        title
    }
}

impl FeatureState {
    fn hydrate_conversation_metadata(&self, value: Value) -> Result<(), MukeiError> {
        let records: Vec<ConversationRecord> =
            serde_json::from_value(value).map_err(|_| MukeiError::DatabaseCorruption)?;
        let mut hydrated = HashMap::new();
        for record in records {
            if record.conversation_id.trim().is_empty()
                || record.active_branch_id.trim().is_empty()
                || record.title.trim().is_empty()
                || hydrated.contains_key(&record.conversation_id)
            {
                return Err(MukeiError::DatabaseCorruption);
            }
            hydrated.insert(record.conversation_id.clone(), record);
        }
        *self
            .conversation_records
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = hydrated;
        Ok(())
    }

    fn reconcile_conversation_records(&self) -> Result<(), MukeiError> {
        let conversations = self
            .conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let mut bindings = self
            .conversation_projects
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut records = self
            .conversation_records
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        for record in records.values() {
            match (bindings.get(&record.conversation_id), record.project_id.as_ref()) {
                (Some(existing), Some(project_id)) if existing != project_id => {
                    return Err(MukeiError::DatabaseCorruption)
                }
                (Some(_), None) => return Err(MukeiError::DatabaseCorruption),
                (None, Some(project_id)) => {
                    bindings.insert(record.conversation_id.clone(), project_id.clone());
                }
                _ => {}
            }
            let has_any_branch = conversations
                .keys()
                .any(|(conversation_id, _)| conversation_id == &record.conversation_id);
            if has_any_branch
                && !conversations.contains_key(&(
                    record.conversation_id.clone(),
                    record.active_branch_id.clone(),
                ))
            {
                return Err(MukeiError::DatabaseCorruption);
            }
        }

        let mut conversation_ids = conversations
            .keys()
            .map(|(conversation_id, _)| conversation_id.clone())
            .collect::<Vec<_>>();
        conversation_ids.sort();
        conversation_ids.dedup();

        for conversation_id in conversation_ids {
            if records.contains_key(&conversation_id) {
                continue;
            }
            let mut branches = conversations
                .iter()
                .filter(|((candidate, _), _)| candidate == &conversation_id)
                .map(|((_, branch_id), messages)| (branch_id.clone(), messages.clone()))
                .collect::<Vec<_>>();
            branches.sort_by(|left, right| left.0.cmp(&right.0));

            let mut all_messages = branches
                .iter()
                .flat_map(|(_, messages)| messages.iter())
                .collect::<Vec<_>>();
            all_messages.sort_by(|left, right| {
                left.created_at
                    .cmp(&right.created_at)
                    .then_with(|| left.id.0.cmp(&right.id.0))
            });
            let title = all_messages
                .iter()
                .find(|message| message.role == Role::User)
                .map(|message| conversation_title_from_text(&message.content))
                .unwrap_or_else(|| "New conversation".to_owned());
            let created_at = all_messages
                .first()
                .map(|message| message.created_at)
                .unwrap_or_else(Utc::now);
            let updated_at = all_messages
                .last()
                .map(|message| message.created_at)
                .unwrap_or(created_at);
            let active_branch_id = branches
                .iter()
                .max_by(|left, right| {
                    let left_time = left.1.last().map(|message| message.created_at);
                    let right_time = right.1.last().map(|message| message.created_at);
                    left_time
                        .cmp(&right_time)
                        .then_with(|| left.0.cmp(&right.0))
                })
                .map(|(branch_id, _)| branch_id.clone())
                .ok_or(MukeiError::DatabaseCorruption)?;
            records.insert(
                conversation_id.clone(),
                ConversationRecord {
                    conversation_id: conversation_id.clone(),
                    title,
                    project_id: bindings.get(&conversation_id).cloned(),
                    active_branch_id,
                    status: ConversationStatus::Active,
                    created_at,
                    updated_at,
                },
            );
        }
        drop(records);
        drop(bindings);
        self.persist_conversation_metadata();
        Ok(())
    }

    fn persist_conversation_metadata(&self) {
        let _enqueue = self
            .persistence_enqueue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut records = self
            .conversation_records
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.conversation_id.cmp(&right.conversation_id))
        });
        if let Ok(value) = serde_json::to_value(records) {
            self.persist_value("conversation_metadata", value);
        }
    }

    fn conversation_metadata_snapshot(&self) -> Vec<ConversationRecord> {
        let mut records = self
            .conversation_records
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.conversation_id.cmp(&right.conversation_id))
        });
        records
    }

    fn conversation_record(&self, conversation_id: &str) -> Option<ConversationRecord> {
        self.conversation_records
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(conversation_id)
            .cloned()
    }

    fn ensure_active_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationRecord, RejectionReason> {
        let record = self
            .conversation_record(conversation_id)
            .ok_or(RejectionReason::StaleScope)?;
        if record.status != ConversationStatus::Active {
            return Err(RejectionReason::PolicyDenied);
        }
        Ok(record)
    }

    fn prepare_conversation_for_send(
        &self,
        conversation_id: &str,
        branch_id: &str,
        project_id: Option<&str>,
        first_text: &str,
    ) -> Result<(), RejectionReason> {
        if let Some(existing) = self.conversation_record(conversation_id) {
            if existing.status != ConversationStatus::Active {
                return Err(RejectionReason::PolicyDenied);
            }
            if project_id.is_some() {
                return Err(RejectionReason::PolicyDenied);
            }
            self.touch_conversation(conversation_id, branch_id);
            self.persist_conversation_metadata();
            return Ok(());
        }

        if self
            .conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .any(|(candidate, _)| candidate == conversation_id)
        {
            return Err(RejectionReason::StaleScope);
        }

        if let Some(project_id) = project_id {
            self.bind_conversation_project(conversation_id, project_id)?;
        }
        let now = Utc::now();
        let record = ConversationRecord {
            conversation_id: conversation_id.to_owned(),
            title: conversation_title_from_text(first_text),
            project_id: self
                .conversation_projects
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(conversation_id)
                .cloned(),
            active_branch_id: branch_id.to_owned(),
            status: ConversationStatus::Active,
            created_at: now,
            updated_at: now,
        };
        self.conversation_records
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(conversation_id.to_owned(), record);
        self.persist_conversation_metadata();
        Ok(())
    }

    fn touch_conversation(&self, conversation_id: &str, branch_id: &str) {
        if let Some(record) = self
            .conversation_records
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get_mut(conversation_id)
        {
            record.active_branch_id = branch_id.to_owned();
            record.updated_at = Utc::now();
        }
    }

    fn conversation_busy(&self, conversation_id: &str) -> bool {
        self.operations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .any(|operation| {
                matches!(operation.status, OperationStatus::Accepted | OperationStatus::Running)
                    && operation
                        .scope
                        .as_ref()
                        .and_then(|scope| scope.conversation_id.as_deref())
                        == Some(conversation_id)
            })
    }

    fn rename_conversation_record(
        &self,
        conversation_id: &str,
        title: &str,
    ) -> Result<ConversationRecord, RejectionReason> {
        let updated = {
            let mut records = self
                .conversation_records
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let record = records
                .get_mut(conversation_id)
                .ok_or(RejectionReason::StaleScope)?;
            record.title = title.trim().to_owned();
            record.updated_at = Utc::now();
            record.clone()
        };
        self.persist_conversation_metadata();
        Ok(updated)
    }

    fn archive_conversation_record(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationRecord, RejectionReason> {
        if self.conversation_busy(conversation_id) {
            return Err(RejectionReason::BusyConflict);
        }
        let updated = {
            let mut records = self
                .conversation_records
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let record = records
                .get_mut(conversation_id)
                .ok_or(RejectionReason::StaleScope)?;
            record.status = ConversationStatus::Archived;
            record.updated_at = Utc::now();
            record.clone()
        };
        self.persist_conversation_metadata();
        Ok(updated)
    }

    fn select_conversation_branch(
        &self,
        conversation_id: &str,
        branch_id: &str,
    ) -> Result<ConversationRecord, RejectionReason> {
        if !self
            .conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&(conversation_id.to_owned(), branch_id.to_owned()))
        {
            return Err(RejectionReason::StaleScope);
        }
        let updated = {
            let mut records = self
                .conversation_records
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let record = records
                .get_mut(conversation_id)
                .ok_or(RejectionReason::StaleScope)?;
            record.active_branch_id = branch_id.to_owned();
            record.clone()
        };
        self.persist_conversation_metadata();
        Ok(updated)
    }

    fn delete_conversation_record(&self, conversation_id: &str) -> Result<usize, RejectionReason> {
        if self.conversation_busy(conversation_id) {
            return Err(RejectionReason::BusyConflict);
        }
        if self
            .conversation_records
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(conversation_id)
            .is_none()
        {
            return Err(RejectionReason::StaleScope);
        }
        self.conversation_projects
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(conversation_id);
        let removed_messages = {
            let mut conversations = self
                .conversations
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let keys = conversations
                .keys()
                .filter(|(candidate, _)| candidate == conversation_id)
                .cloned()
                .collect::<Vec<_>>();
            let mut removed_messages = 0usize;
            for key in keys {
                removed_messages += conversations.remove(&key).map_or(0, |messages| messages.len());
            }
            removed_messages
        };
        self.persist_conversations();
        self.persist_conversation_metadata();
        Ok(removed_messages)
    }
}

impl MukeiRuntime {
    fn parse_conversation_scope(
        command: &ValidatedCommand,
    ) -> Result<String, CommandAcknowledgementV2> {
        let conversation = command
            .envelope
            .scope
            .as_ref()
            .and_then(|scope| scope.conversation_id.as_deref())
            .ok_or_else(|| {
                CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::StaleScope,
                )
            })?;
        Ok(conversation.to_owned())
    }

    fn complete_conversation_operation(
        &self,
        command: &ValidatedCommand,
        operation_id: &str,
        event_type: &str,
        result: Value,
    ) {
        self.features.update_operation(
            operation_id,
            OperationStatus::Completed,
            Some(1.0),
            None,
            result.clone(),
        );
        let conversation_id = command
            .envelope
            .scope
            .as_ref()
            .and_then(|scope| scope.conversation_id.as_deref())
            .unwrap_or("unknown");
        self.events.emit(
            &format!("conversation:{conversation_id}"),
            event_type,
            result,
            Some(&command.envelope),
            Some(operation_id.to_owned()),
        );
    }

    fn rename_conversation(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(acknowledgement) = self.ensure_ready(command) {
            return acknowledgement;
        }
        let ValidatedCommandPayload::ConversationRename(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        let conversation = match Self::parse_conversation_scope(command) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        let updated = match self
            .features
            .rename_conversation_record(&conversation, &payload.title)
        {
            Ok(value) => value,
            Err(reason) => {
                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
            }
        };
        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        self.complete_conversation_operation(
            command,
            &operation_id,
            "conversation.renamed",
            json!({
                "conversation_id": updated.conversation_id,
                "title": updated.title,
                "updated_at": updated.updated_at,
            }),
        );
        acknowledgement
    }

    fn archive_conversation(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(acknowledgement) = self.ensure_ready(command) {
            return acknowledgement;
        }
        let conversation = match Self::parse_conversation_scope(command) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        let updated = match self.features.archive_conversation_record(&conversation) {
            Ok(value) => value,
            Err(reason) => {
                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
            }
        };
        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        self.complete_conversation_operation(
            command,
            &operation_id,
            "conversation.archived",
            json!({
                "conversation_id": updated.conversation_id,
                "status": updated.status,
                "updated_at": updated.updated_at,
            }),
        );
        acknowledgement
    }

    fn delete_conversation(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(acknowledgement) = self.ensure_ready(command) {
            return acknowledgement;
        }
        let conversation = match Self::parse_conversation_scope(command) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        #[cfg(feature = "rusqlite")]
        {
            if self.features.conversation_record(&conversation).is_none() {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::StaleScope,
                );
            }
            if self.features.conversation_busy(&conversation) {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::BusyConflict,
                );
            }
            if let Some(service) = self
                .conversation_attachments
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
            {
                if self
                    .async_runtime
                    .block_on(service.remove_all_for_conversation(conversation.clone()))
                    .is_err()
                {
                    return CommandAcknowledgementV2::rejected(
                        Some(&command.envelope),
                        RejectionReason::BackendUnavailable,
                    );
                }
            }
        }
        let removed_messages = match self.features.delete_conversation_record(&conversation) {
            Ok(value) => value,
            Err(reason) => {
                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
            }
        };
        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        self.complete_conversation_operation(
            command,
            &operation_id,
            "conversation.deleted",
            json!({
                "conversation_id": conversation,
                "removed_messages": removed_messages,
            }),
        );
        acknowledgement
    }

    fn select_active_conversation_branch(
        &self,
        command: &ValidatedCommand,
    ) -> CommandAcknowledgementV2 {
        if let Err(acknowledgement) = self.ensure_ready(command) {
            return acknowledgement;
        }
        let (conversation, branch, _, _) = match Self::parse_chat_scope(command) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        let updated = match self
            .features
            .select_conversation_branch(&conversation, &branch)
        {
            Ok(value) => value,
            Err(reason) => {
                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
            }
        };
        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        self.complete_conversation_operation(
            command,
            &operation_id,
            "conversation.branch.selected",
            json!({
                "conversation_id": updated.conversation_id,
                "active_branch_id": updated.active_branch_id,
            }),
        );
        acknowledgement
    }
}

#[cfg(test)]
mod actual_conversation_tests {
    use super::*;

    #[tokio::test]
    async fn first_send_creates_first_class_conversation_metadata() {
        let state = FeatureState::new(tokio::runtime::Handle::current());
        let conversation = Uuid::new_v4().to_string();
        let branch = Uuid::new_v4().to_string();
        state
            .prepare_conversation_for_send(
                &conversation,
                &branch,
                None,
                "  Build an offline notes app\nwith sync later",
            )
            .expect("create conversation");
        let record = state.conversation_record(&conversation).expect("record");
        assert_eq!(record.title, "Build an offline notes app");
        assert_eq!(record.active_branch_id, branch);
        assert_eq!(record.status, ConversationStatus::Active);
    }

    #[tokio::test]
    async fn archived_conversation_is_read_only() {
        let state = FeatureState::new(tokio::runtime::Handle::current());
        let conversation = Uuid::new_v4().to_string();
        let branch = Uuid::new_v4().to_string();
        state
            .prepare_conversation_for_send(&conversation, &branch, None, "Hello")
            .expect("create conversation");
        state
            .archive_conversation_record(&conversation)
            .expect("archive");
        assert!(matches!(
            state.ensure_active_conversation(&conversation),
            Err(RejectionReason::PolicyDenied)
        ));
    }

    #[tokio::test]
    async fn active_branch_selection_is_persisted_in_metadata() {
        let state = FeatureState::new(tokio::runtime::Handle::current());
        let conversation = Uuid::new_v4().to_string();
        let branch_a = BranchId::new();
        let branch_b = BranchId::new();
        state
            .prepare_conversation_for_send(
                &conversation,
                &branch_a.0.to_string(),
                None,
                "Hello",
            )
            .expect("create conversation");
        state.append_message(
            &conversation,
            &branch_a.0.to_string(),
            ChatMessage::user_with_id(MessageId::new(), branch_a, "one"),
        );
        state.append_message(
            &conversation,
            &branch_b.0.to_string(),
            ChatMessage::user_with_id(MessageId::new(), branch_b, "two"),
        );
        state
            .select_conversation_branch(&conversation, &branch_a.0.to_string())
            .expect("select branch");
        assert_eq!(
            state
                .conversation_record(&conversation)
                .expect("record")
                .active_branch_id,
            branch_a.0.to_string()
        );
    }
}
