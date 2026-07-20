from pathlib import Path


def read(path: str) -> str:
    return Path(path).read_text()


def write(path: str, value: str) -> None:
    Path(path).write_text(value)


def replace_once(path: str, old: str, new: str) -> None:
    value = read(path)
    if old not in value:
        raise SystemExit(f"anchor missing in {path}: {old[:120]!r}")
    if value.count(old) != 1:
        raise SystemExit(f"anchor not unique in {path}: {value.count(old)} matches")
    write(path, value.replace(old, new, 1))


# ---- Rust foundation types/state -------------------------------------------------
path = "rust/crates/mukei-core/src/application_runtime/foundation_types.rs"
replace_once(
    path,
    '''#[derive(Clone, Debug, Serialize, Deserialize)]
struct ConversationProjection {
    conversation_id: String,
    branch_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    messages: Vec<ChatMessage>,
}
''',
    '''#[derive(Clone, Debug, Serialize, Deserialize)]
struct ConversationProjection {
    conversation_id: String,
    branch_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    messages: Vec<ChatMessage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConversationStatus {
    Active,
    Archived,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ConversationRecord {
    conversation_id: String,
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    active_branch_id: String,
    status: ConversationStatus,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}
''',
)
replace_once(
    path,
    '''    conversations: RwLock<HashMap<(String, String), Vec<ChatMessage>>>,
    conversation_projects: RwLock<HashMap<String, String>>,
''',
    '''    conversations: RwLock<HashMap<(String, String), Vec<ChatMessage>>>,
    conversation_projects: RwLock<HashMap<String, String>>,
    conversation_records: RwLock<HashMap<String, ConversationRecord>>,
''',
)
replace_once(
    path,
    '''            conversations: RwLock::new(HashMap::new()),
            conversation_projects: RwLock::new(HashMap::new()),
''',
    '''            conversations: RwLock::new(HashMap::new()),
            conversation_projects: RwLock::new(HashMap::new()),
            conversation_records: RwLock::new(HashMap::new()),
''',
)

path = "rust/crates/mukei-core/src/application_runtime/foundation_state.rs"
replace_once(
    path,
    '''        if let Some(value) = store.load("projects").await? {
''',
    '''        if let Some(value) = store.load("conversation_metadata").await? {
            self.hydrate_conversation_metadata(value)?;
        }
        self.reconcile_conversation_records()?;
        if let Some(value) = store.load("projects").await? {
''',
)
replace_once(
    path,
    '''    fn append_message(&self, conversation: &str, branch: &str, message: ChatMessage) {
        self.conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry((conversation.to_owned(), branch.to_owned()))
            .or_default()
            .push(message);
        self.persist_conversations();
    }
''',
    '''    fn append_message(&self, conversation: &str, branch: &str, message: ChatMessage) {
        self.conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry((conversation.to_owned(), branch.to_owned()))
            .or_default()
            .push(message);
        self.touch_conversation(conversation, branch);
        self.persist_conversations();
        self.persist_conversation_metadata();
    }
''',
)
replace_once(
    path,
    '''    fn clear_conversation(&self, conversation: &str, branch: &str) -> usize {
        let removed = self
            .conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&(conversation.to_owned(), branch.to_owned()))
            .map(|messages| messages.len())
            .unwrap_or(0);
        self.persist_conversations();
        removed
    }
''',
    '''    fn clear_conversation(&self, conversation: &str, branch: &str) -> usize {
        let removed = {
            let mut conversations = self
                .conversations
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let messages = conversations
                .entry((conversation.to_owned(), branch.to_owned()))
                .or_default();
            let removed = messages.len();
            messages.clear();
            removed
        };
        self.touch_conversation(conversation, branch);
        self.persist_conversations();
        self.persist_conversation_metadata();
        removed
    }
''',
)

# ---- New first-class conversation runtime ---------------------------------------
conversation_rs = r'''const MAX_CONVERSATION_TITLE_CHARS: usize = 128;

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
'''
write("rust/crates/mukei-core/src/application_runtime/conversation.rs", conversation_rs)

path = "rust/crates/mukei-core/src/application_runtime.rs"
replace_once(
    path,
    '''include!("application_runtime/chat_snapshot.rs");
''',
    '''include!("application_runtime/chat_snapshot.rs");
include!("application_runtime/conversation.rs");
''',
)

# ---- Conversation metadata persistence and snapshots -----------------------------
path = "rust/crates/mukei-core/src/application_runtime/persistence_flush.rs"
replace_once(
    path,
    '''            let conversations = self
                .conversations
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .map(
                    |((conversation_id, branch_id), messages)| ConversationProjection {
                        conversation_id: conversation_id.clone(),
                        branch_id: branch_id.clone(),
                        messages: messages.clone(),
                    },
                )
                .collect::<Vec<_>>();
''',
    '''            let bindings = self
                .conversation_projects
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            let conversations = self
                .conversations
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .map(
                    |((conversation_id, branch_id), messages)| ConversationProjection {
                        conversation_id: conversation_id.clone(),
                        branch_id: branch_id.clone(),
                        project_id: bindings.get(conversation_id).cloned(),
                        messages: messages.clone(),
                    },
                )
                .collect::<Vec<_>>();
            let conversation_metadata = self
                .conversation_records
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .values()
                .cloned()
                .collect::<Vec<_>>();
''',
)
replace_once(
    path,
    '''                (
                    "projects",
                    serde_json::to_value(projects)
                        .map_err(|error| MukeiError::Internal(error.to_string()))?,
                ),
''',
    '''                (
                    "conversation_metadata",
                    serde_json::to_value(conversation_metadata)
                        .map_err(|error| MukeiError::Internal(error.to_string()))?,
                ),
                (
                    "projects",
                    serde_json::to_value(projects)
                        .map_err(|error| MukeiError::Internal(error.to_string()))?,
                ),
''',
)

path = "rust/crates/mukei-core/src/application_runtime/chat_branching.rs"
replace_once(
    path,
    '''        json!({ "branches": branches })
''',
    '''        json!({
            "conversations": self.conversation_metadata_snapshot(),
            "branches": branches,
        })
''',
)
replace_once(
    path,
    '''        self.persist_conversations();
        let target = cloned
''',
    '''        self.touch_conversation(conversation, &new_branch);
        self.persist_conversations();
        self.persist_conversation_metadata();
        let target = cloned
''',
)
replace_once(
    path,
    '''        let message_id = match Uuid::parse_str(message_id) {
''',
    '''        if let Err(reason) = self.features.ensure_active_conversation(&conversation) {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason);
        }
        let message_id = match Uuid::parse_str(message_id) {
''',
)
replace_once(
    path,
    '''        let Some(user_message) = self.features.last_user_message(&conversation, &source_branch) else {
''',
    '''        if let Err(reason) = self.features.ensure_active_conversation(&conversation) {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason);
        }
        let Some(user_message) = self.features.last_user_message(&conversation, &source_branch) else {
''',
)

path = "rust/crates/mukei-core/src/application_runtime/chat_snapshot.rs"
replace_once(
    path,
    '''            let branches = self
                .conversations_snapshot()
                .get("branches")
                .cloned()
                .unwrap_or_else(|| json!([]));
            object.insert("conversation_branches".to_owned(), branches);
''',
    '''            let conversations = self.conversations_snapshot();
            let branches = conversations
                .get("branches")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let metadata = conversations
                .get("conversations")
                .cloned()
                .unwrap_or_else(|| json!([]));
            object.insert("conversation_branches".to_owned(), branches);
            object.insert("conversations".to_owned(), metadata);
''',
)

# ---- Chat entry points enforce conversation lifecycle ----------------------------
path = "rust/crates/mukei-core/src/application_runtime/chat.rs"
old = '''        if let Some(message_id) = command
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
'''
new = '''        if let Some(message_id) = command
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
        if let Err(acknowledgement) = self.ensure_inference_ready_for_branching(command) {
            return acknowledgement;
        }
        let (conversation, branch, _, _) = match Self::parse_chat_scope(command) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        if let Err(reason) = self.features.prepare_conversation_for_send(
            &conversation,
            &branch,
            payload.project_id.as_deref(),
            &payload.text,
        ) {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason);
        }
        self.start_chat_operation(command, payload.text.clone(), false, None)
'''
replace_once(path, old, new)
replace_once(
    path,
    '''        if !self.activation.readiness_snapshot().active_backend_ready {
''',
    '''        if let Err(reason) = self.features.ensure_active_conversation(&conversation) {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason);
        }
        if !self.activation.readiness_snapshot().active_backend_ready {
''',
)

# ---- Protocol: lifecycle commands ------------------------------------------------
path = "rust/crates/mukei-core/src/ui_protocol.rs"
replace_once(
    path,
    '''    /// Clear the active conversation.
    ChatClearConversation,
    /// Start a model download.
''',
    '''    /// Clear messages from the active conversation branch without deleting its identity.
    ChatClearConversation,
    /// Rename one durable conversation.
    ConversationRename,
    /// Archive one durable conversation and make it read-only.
    ConversationArchive,
    /// Permanently delete one durable conversation and all of its branches.
    ConversationDelete,
    /// Persist the active branch selected for one conversation.
    ConversationSelectBranch,
    /// Start a model download.
''',
)
replace_once(
    path,
    '''            "chat.clear_conversation" => Some(Self::ChatClearConversation),
''',
    '''            "chat.clear_conversation" => Some(Self::ChatClearConversation),
            "conversation.rename" => Some(Self::ConversationRename),
            "conversation.archive" => Some(Self::ConversationArchive),
            "conversation.delete" => Some(Self::ConversationDelete),
            "conversation.select_branch" => Some(Self::ConversationSelectBranch),
''',
)
replace_once(
    path,
    '''            Self::ChatClearConversation => "chat.clear_conversation",
''',
    '''            Self::ChatClearConversation => "chat.clear_conversation",
            Self::ConversationRename => "conversation.rename",
            Self::ConversationArchive => "conversation.archive",
            Self::ConversationDelete => "conversation.delete",
            Self::ConversationSelectBranch => "conversation.select_branch",
''',
)
replace_once(
    path,
    '''            Self::ChatSendMessage
                | Self::ModelDownload
''',
    '''            Self::ChatSendMessage
                | Self::ConversationRename
                | Self::ConversationArchive
                | Self::ConversationDelete
                | Self::ConversationSelectBranch
                | Self::ModelDownload
''',
)
replace_once(
    path,
    '''pub struct SendMessagePayload {
    /// User-authored text.
    pub text: String,
    /// Optional active project to bind when creating a brand-new conversation.
    #[serde(default)]
    pub project_id: Option<String>,
}
''',
    '''pub struct SendMessagePayload {
    /// User-authored text.
    pub text: String,
    /// Optional active project to bind when creating a brand-new conversation.
    #[serde(default)]
    pub project_id: Option<String>,
}

/// Mutable conversation title payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationRenamePayload {
    /// Replacement user-visible title.
    pub title: String,
}
''',
)
replace_once(
    path,
    '''    /// Chat submission.
    SendMessage(SendMessagePayload),
''',
    '''    /// Chat submission.
    SendMessage(SendMessagePayload),
    /// Conversation title mutation.
    ConversationRename(ConversationRenamePayload),
''',
)
replace_once(
    path,
    '''            CommandType::RecoveryResume
                | CommandType::RecoveryRegenerate
                | CommandType::StorageImportFile
''',
    '''            CommandType::RecoveryResume
                | CommandType::RecoveryRegenerate
                | CommandType::StorageImportFile
                | CommandType::ConversationRename
                | CommandType::ConversationArchive
                | CommandType::ConversationDelete
                | CommandType::ConversationSelectBranch
''',
)
replace_once(
    path,
    '''        (CommandType::ChatStopGeneration, _) => {
''',
    '''        (
            CommandType::ConversationRename
            | CommandType::ConversationArchive
            | CommandType::ConversationDelete,
            _,
        ) => {
            if scope.conversation_id.is_none()
                || scope.branch_id.is_some()
                || scope.turn_id.is_some()
                || has_model
                || has_document
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (CommandType::ConversationSelectBranch, _) => {
            if scope.conversation_id.is_none()
                || scope.branch_id.is_none()
                || scope.turn_id.is_some()
                || has_model
                || has_document
            {
                return Err(RejectionReason::StaleScope);
            }
        }
        (CommandType::ChatStopGeneration, _) => {
''',
)
replace_once(
    path,
    '''        CommandType::ModelDownload => {
''',
    '''        CommandType::ConversationRename => {
            let value: ConversationRenamePayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !non_empty_bounded(&value.title, 128)
                || value.title.chars().any(char::is_control)
            {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::ConversationRename(value)
        }
        CommandType::ModelDownload => {
''',
)
replace_once(
    path,
    '''        CommandType::ChatStopGeneration
        | CommandType::ChatClearConversation
        | CommandType::DownloadCancel
''',
    '''        CommandType::ChatStopGeneration
        | CommandType::ChatClearConversation
        | CommandType::ConversationArchive
        | CommandType::ConversationDelete
        | CommandType::ConversationSelectBranch
        | CommandType::DownloadCancel
''',
)

# ---- Router/capabilities ---------------------------------------------------------
path = "rust/crates/mukei-core/src/application_runtime/foundation_context.rs"
replace_once(
    path,
    '''            CommandType::ChatClearConversation => runtime.clear_conversation(command),
''',
    '''            CommandType::ChatClearConversation => runtime.clear_conversation(command),
            CommandType::ConversationRename => runtime.rename_conversation(command),
            CommandType::ConversationArchive => runtime.archive_conversation(command),
            CommandType::ConversationDelete => runtime.delete_conversation(command),
            CommandType::ConversationSelectBranch => runtime.select_active_conversation_branch(command),
''',
)

path = "rust/crates/mukei-core/src/application_runtime/base.rs"
replace_once(
    path,
    '''            CommandType::ChatClearConversation,
            CommandType::DownloadCancel,
''',
    '''            CommandType::ChatClearConversation,
            CommandType::ConversationRename,
            CommandType::ConversationArchive,
            CommandType::ConversationDelete,
            CommandType::ConversationSelectBranch,
            CommandType::DownloadCancel,
''',
)

# ---- Android bridge --------------------------------------------------------------
path = "android/app/src/main/kotlin/ai/mukei/android/BackendRuntimeHost.kt"
replace_once(
    path,
    '''    fun regenerateChat(
''',
    '''    fun renameConversation(conversationId: String, title: String): ChatCommandSubmission =
        submitConversationCommand(
            commandType = "conversation.rename",
            conversationId = conversationId,
            payload = JSONObject().put("title", title),
        )

    fun archiveConversation(conversationId: String): ChatCommandSubmission =
        submitConversationCommand(
            commandType = "conversation.archive",
            conversationId = conversationId,
            payload = JSONObject(),
        )

    fun deleteConversation(conversationId: String): ChatCommandSubmission =
        submitConversationCommand(
            commandType = "conversation.delete",
            conversationId = conversationId,
            payload = JSONObject(),
        )

    fun selectConversationBranch(
        conversationId: String,
        branchId: String,
    ): ChatCommandSubmission = submitConversationCommand(
        commandType = "conversation.select_branch",
        conversationId = conversationId,
        branchId = branchId,
        payload = JSONObject(),
    )

    fun regenerateChat(
''',
)
replace_once(
    path,
    '''    private fun submitChatCommand(
''',
    '''    private fun submitConversationCommand(
        commandType: String,
        conversationId: String,
        branchId: String? = null,
        payload: JSONObject,
    ): ChatCommandSubmission {
        val activeGateway = gateway.get()
            ?: return ChatCommandSubmission("rejected", null, "backend_unavailable")
        if (conversationId.isBlank() || (branchId != null && branchId.isBlank())) {
            return ChatCommandSubmission("rejected", null, "stale_scope")
        }
        return try {
            val scope = JSONObject().put("conversation_id", conversationId)
            if (branchId != null) scope.put("branch_id", branchId)
            val envelope = JSONObject()
                .put("protocol_version", JSONObject().put("major", 2).put("minor", 3))
                .put("command_id", UUID.randomUUID().toString())
                .put("request_id", UUID.randomUUID().toString())
                .put("command_type", commandType)
                .put("submitted_at", Instant.now().toString())
                .put("correlation_id", UUID.randomUUID().toString())
                .put("idempotency_key", "conversation-${UUID.randomUUID()}")
                .put("scope", scope)
                .put("payload", payload)
            val acknowledgement = JSONObject(
                String(
                    activeGateway.submitCommand(
                        envelope.toString().toByteArray(StandardCharsets.UTF_8),
                    ),
                    StandardCharsets.UTF_8,
                ),
            )
            ChatCommandSubmission(
                status = acknowledgement.optString("status", "rejected"),
                operationId = acknowledgement.optString("operation_id").takeIf { it.isNotBlank() },
                rejectionReason = acknowledgement.optString("rejection_reason")
                    .takeIf { it.isNotBlank() },
            )
        } catch (failure: Throwable) {
            ChatCommandSubmission("rejected", null, stableFailureCode(failure))
        }
    }

    private fun submitChatCommand(
''',
)
replace_once(
    path,
    '''            val branches = payload.optJSONArray("conversation_branches")
            envelope.put("payload", JSONObject().put("branches", branches))
''',
    '''            val branches = payload.optJSONArray("conversation_branches")
            val conversations = payload.optJSONArray("conversations")
            envelope.put(
                "payload",
                JSONObject()
                    .put("conversations", conversations)
                    .put("branches", branches),
            )
''',
)

# ---- Android conversation workspace integrated into real navigation --------------
workspace_kt = r'''package ai.mukei.android

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import ai.mukei.android.designsystem.MukeiSpacing
import java.util.UUID

@Composable
internal fun ConversationWorkspaceSurface(
    readiness: AppReadiness,
    resetGeneration: Int,
    initialPrompt: String?,
    onInitialPromptConsumed: () -> Unit,
) {
    var conversationId by remember { mutableStateOf<String?>(null) }
    var branchId by remember { mutableStateOf<String?>(null) }
    var initialOperationId by remember { mutableStateOf<String?>(null) }
    var draft by remember { mutableStateOf("") }
    var banner by remember { mutableStateOf<String?>(null) }
    var selectedProjectId by remember { mutableStateOf<String?>(null) }

    fun resetToNewConversation() {
        conversationId = null
        branchId = null
        initialOperationId = null
        draft = ""
        banner = null
        selectedProjectId = null
    }

    fun startConversation(text: String, projectId: String?) {
        val newConversation = UUID.randomUUID().toString()
        val newBranch = UUID.randomUUID().toString()
        val result = BackendRuntimeHost.sendChatMessage(
            conversationId = newConversation,
            branchId = newBranch,
            text = text,
            projectId = projectId,
        )
        if (result.status == "accepted") {
            conversationId = newConversation
            branchId = newBranch
            initialOperationId = result.operationId
            draft = ""
            banner = null
            selectedProjectId = null
        } else {
            banner = when (result.rejectionReason) {
                "backend_unavailable" -> "A ready model is required before sending."
                "policy_denied" -> "That project cannot be attached to this conversation."
                "stale_scope" -> "That project or conversation scope is no longer available."
                else -> "Message could not be sent: ${result.rejectionReason ?: "rejected"}"
            }
        }
    }

    LaunchedEffect(resetGeneration) {
        if (resetGeneration > 0) resetToNewConversation()
    }
    LaunchedEffect(initialPrompt) {
        val prompt = initialPrompt?.trim().orEmpty()
        if (prompt.isNotEmpty()) {
            onInitialPromptConsumed()
            startConversation(prompt, null)
        }
    }

    val activeConversation = conversationId
    val activeBranch = branchId
    Column(modifier = Modifier.fillMaxSize()) {
        if (activeConversation != null && activeBranch != null) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = MukeiSpacing.Medium),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                TextButton(onClick = { resetToNewConversation() }) { Text("All chats") }
                TextButton(onClick = { resetToNewConversation() }) { Text("New conversation") }
            }
            Box(modifier = Modifier.weight(1f)) {
                ChatConversationSurface(
                    conversationId = activeConversation,
                    branchId = activeBranch,
                    readiness = readiness,
                    initialOperationId = initialOperationId,
                    onBranchChange = { selectedBranch ->
                        val result = BackendRuntimeHost.selectConversationBranch(
                            activeConversation,
                            selectedBranch,
                        )
                        if (result.status == "accepted") {
                            branchId = selectedBranch
                            initialOperationId = null
                        } else {
                            banner = "Branch could not be opened: ${result.rejectionReason ?: "rejected"}"
                        }
                    },
                )
            }
        } else {
            Box(modifier = Modifier.weight(1f)) {
                ChatsSurface { selectedConversation, selectedBranch ->
                    conversationId = selectedConversation
                    branchId = selectedBranch
                    initialOperationId = null
                    selectedProjectId = null
                    banner = null
                }
            }
            banner?.let { message ->
                Text(
                    text = message,
                    modifier = Modifier.padding(horizontal = MukeiSpacing.Large),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(MukeiSpacing.Large),
                verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Small),
            ) {
                val projectOptions = loadActiveChatProjects()
                if (projectOptions.isNotEmpty()) {
                    Text("Project context", style = MaterialTheme.typography.labelLarge)
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .horizontalScroll(rememberScrollState()),
                        horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.ExtraSmall),
                    ) {
                        TextButton(onClick = { selectedProjectId = null }) {
                            Text(if (selectedProjectId == null) "None · Selected" else "None")
                        }
                        projectOptions.forEach { project ->
                            TextButton(onClick = { selectedProjectId = project.projectId }) {
                                Text(
                                    if (selectedProjectId == project.projectId) {
                                        "${project.name} · Selected"
                                    } else {
                                        project.name
                                    },
                                )
                            }
                        }
                    }
                    Text(
                        "Project binding is fixed when the conversation is first created.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                OutlinedTextField(
                    value = draft,
                    onValueChange = { draft = it.take(64 * 1024) },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text("Message Mukei") },
                    minLines = 2,
                    maxLines = 6,
                )
                Button(
                    onClick = { startConversation(draft.trim(), selectedProjectId) },
                    enabled = readiness.inference.status == ReadinessStatus.READY &&
                        draft.trim().isNotEmpty(),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Start conversation")
                }
            }
        }
    }
}
'''
write("android/app/src/main/kotlin/ai/mukei/android/ConversationWorkspaceSurface.kt", workspace_kt)

# MukeiAppShell becomes a thin compatibility entry point; no overlay navigation.
write(
    "android/app/src/main/kotlin/ai/mukei/android/MukeiAppShell.kt",
    '''package ai.mukei.android\n\nimport androidx.compose.runtime.Composable\n\n@Composable\ninternal fun MukeiAppShell(backendState: BackendRuntimeHost.State) {\n    MukeiProductShell(backendState)\n}\n''',
)

# Product shell: real Chats destination and Home composer routing.
path = "android/app/src/main/kotlin/ai/mukei/android/MukeiProductShell.kt"
replace_once(
    path,
    '''    var newChatGeneration by rememberSaveable { mutableIntStateOf(0) }
''',
    '''    var newChatGeneration by rememberSaveable { mutableIntStateOf(0) }
    var conversationWorkspaceGeneration by rememberSaveable { mutableIntStateOf(0) }
    var pendingHomePrompt by rememberSaveable { mutableStateOf<String?>(null) }
''',
)
replace_once(
    path,
    '''                            onClick = {
                                selectedName = TopLevelDestination.HOME.name
                                newChatGeneration += 1
                            },
''',
    '''                            onClick = {
                                selectedName = TopLevelDestination.CHATS.name
                                pendingHomePrompt = null
                                conversationWorkspaceGeneration += 1
                            },
''',
)
replace_once(
    path,
    '''                    TopLevelDestination.HOME -> HomeSurface(
                        readiness = state.readiness,
                        resetGeneration = newChatGeneration,
                        openModels = { selectedName = TopLevelDestination.MODELS.name },
                    )
                    TopLevelDestination.STORAGE -> StorageSurface()
                    TopLevelDestination.PROJECTS -> ProjectsSurface()
                    TopLevelDestination.MODELS -> ModelsSurface(state.readiness)
                    else -> ReservedDestinationSurface(selected)
''',
    '''                    TopLevelDestination.HOME -> HomeSurface(
                        readiness = state.readiness,
                        resetGeneration = newChatGeneration,
                        openModels = { selectedName = TopLevelDestination.MODELS.name },
                        onStartConversation = { prompt ->
                            pendingHomePrompt = prompt
                            conversationWorkspaceGeneration += 1
                            selectedName = TopLevelDestination.CHATS.name
                        },
                    )
                    TopLevelDestination.STORAGE -> StorageSurface()
                    TopLevelDestination.PROJECTS -> ProjectsSurface()
                    TopLevelDestination.MODELS -> ModelsSurface(state.readiness)
                    TopLevelDestination.CHATS -> ConversationWorkspaceSurface(
                        readiness = state.readiness,
                        resetGeneration = conversationWorkspaceGeneration,
                        initialPrompt = pendingHomePrompt,
                        onInitialPromptConsumed = { pendingHomePrompt = null },
                    )
                    else -> ReservedDestinationSurface(selected)
''',
)
replace_once(
    path,
    '''private fun HomeSurface(
    readiness: AppReadiness,
    resetGeneration: Int,
    openModels: () -> Unit,
) {
''',
    '''private fun HomeSurface(
    readiness: AppReadiness,
    resetGeneration: Int,
    openModels: () -> Unit,
    onStartConversation: (String) -> Unit,
) {
''',
)
replace_once(
    path,
    '''            MukeiComposer(
                draft = draft,
                onDraftChange = { draft = it },
                placeholder = selectedCapability?.placeholder ?: "Tell Mukei what you want to do…",
            )
''',
    '''            MukeiComposer(
                draft = draft,
                onDraftChange = { draft = it },
                placeholder = selectedCapability?.placeholder ?: "Tell Mukei what you want to do…",
                sendEnabled = readiness.inference.status == ReadinessStatus.READY &&
                    draft.trim().isNotEmpty(),
                onSend = {
                    val prompt = draft.trim()
                    if (prompt.isNotEmpty()) {
                        draft = ""
                        onStartConversation(prompt)
                    }
                },
            )
''',
)
replace_once(
    path,
    '''private fun MukeiComposer(
    draft: String,
    onDraftChange: (String) -> Unit,
    placeholder: String,
) {
''',
    '''private fun MukeiComposer(
    draft: String,
    onDraftChange: (String) -> Unit,
    placeholder: String,
    sendEnabled: Boolean,
    onSend: () -> Unit,
) {
''',
)
replace_once(
    path,
    '''                    IconButton(
                        onClick = {},
                        enabled = false,
                        modifier = Modifier.semantics {
                            contentDescription = "Send unavailable until conversation runtime is connected"
                        },
''',
    '''                    IconButton(
                        onClick = onSend,
                        enabled = sendEnabled,
                        modifier = Modifier.semantics {
                            contentDescription = "Send message"
                        },
''',
)

# ---- Chat list/conversation UI metadata lifecycle --------------------------------
path = "android/app/src/main/kotlin/ai/mukei/android/ChatConversationSurface.kt"
replace_once(
    path,
    '''private data class ChatSummary(
    val conversationId: String,
    val branchId: String,
    val title: String,
    val preview: String,
    val branchCount: Int,
    val projectId: String?,
    val lastTimestamp: String,
)
''',
    '''private data class ConversationRecordCard(
    val conversationId: String,
    val title: String,
    val projectId: String?,
    val activeBranchId: String,
    val status: String,
    val createdAt: String,
    val updatedAt: String,
)

private data class ChatSummary(
    val conversationId: String,
    val branchId: String,
    val title: String,
    val preview: String,
    val branchCount: Int,
    val projectId: String?,
    val status: String,
    val lastTimestamp: String,
)
''',
)
replace_once(
    path,
    '''    var branches by remember { mutableStateOf(loadChatBranches()) }

    fun refresh() {
        branches = loadChatBranches()
    }
''',
    '''    var branches by remember { mutableStateOf(loadChatBranches()) }
    var conversations by remember { mutableStateOf(loadConversationRecords()) }
    var renameTarget by remember { mutableStateOf<ChatSummary?>(null) }
    var deleteTarget by remember { mutableStateOf<ChatSummary?>(null) }

    fun refresh() {
        branches = loadChatBranches()
        conversations = loadConversationRecords()
    }
''',
)
replace_once(
    path,
    '''                        JSONObject(raw).optString("event_type").startsWith("chat.")
''',
    '''                        val eventType = JSONObject(raw).optString("event_type")
                        eventType.startsWith("chat.") || eventType.startsWith("conversation.")
''',
)
replace_once(
    path,
    '''    val summaries = remember(branches) { summarizeChats(branches) }
''',
    '''    val summaries = remember(branches, conversations) { summarizeChats(branches, conversations) }
''',
)
replace_once(
    path,
    '''                        Text(chat.title, style = MaterialTheme.typography.titleMedium)
''',
    '''                        Text(chat.title, style = MaterialTheme.typography.titleMedium)
                        if (chat.status == "archived") {
                            Text(
                                "Archived · read-only",
                                style = MaterialTheme.typography.labelMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
''',
)
replace_once(
    path,
    '''                        Button(
                            onClick = { onOpenChat(chat.conversationId, chat.branchId) },
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text("Open chat")
                        }
''',
    '''                        Button(
                            onClick = { onOpenChat(chat.conversationId, chat.branchId) },
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text("Open conversation")
                        }
                        Row(horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.ExtraSmall)) {
                            TextButton(onClick = { renameTarget = chat }) { Text("Rename") }
                            if (chat.status == "active") {
                                TextButton(
                                    onClick = {
                                        val result = BackendRuntimeHost.archiveConversation(chat.conversationId)
                                        if (result.status == "accepted") refresh()
                                    },
                                ) { Text("Archive") }
                            }
                            TextButton(onClick = { deleteTarget = chat }) { Text("Delete") }
                        }
''',
)
replace_once(
    path,
    '''    }
}

@Composable
internal fun ChatConversationSurface(
''',
    '''    }

    renameTarget?.let { target ->
        var title by remember(target.conversationId) { mutableStateOf(target.title) }
        AlertDialog(
            onDismissRequest = { renameTarget = null },
            title = { Text("Rename conversation") },
            text = {
                OutlinedTextField(
                    value = title,
                    onValueChange = { title = it.take(128) },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
            },
            confirmButton = {
                TextButton(
                    enabled = title.trim().isNotEmpty(),
                    onClick = {
                        val result = BackendRuntimeHost.renameConversation(
                            target.conversationId,
                            title.trim(),
                        )
                        if (result.status == "accepted") {
                            renameTarget = null
                            refresh()
                        }
                    },
                ) { Text("Save") }
            },
            dismissButton = { TextButton(onClick = { renameTarget = null }) { Text("Cancel") } },
        )
    }

    deleteTarget?.let { target ->
        AlertDialog(
            onDismissRequest = { deleteTarget = null },
            title = { Text("Delete conversation?") },
            text = { Text("This permanently deletes this conversation and all of its branches.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        val result = BackendRuntimeHost.deleteConversation(target.conversationId)
                        if (result.status == "accepted") {
                            deleteTarget = null
                            refresh()
                        }
                    },
                ) { Text("Delete") }
            },
            dismissButton = { TextButton(onClick = { deleteTarget = null }) { Text("Cancel") } },
        )
    }
}

@Composable
internal fun ChatConversationSurface(
''',
)
replace_once(
    path,
    '''    val messages = activeBranch?.messages.orEmpty()
    val activeProjectName = activeBranch?.projectId?.let { loadChatProjectNames()[it] }
    val lastAssistantId = messages.lastOrNull { it.role == "assistant" }?.messageId
    val canGenerate = readiness.inference.status == ReadinessStatus.READY
''',
    '''    val messages = activeBranch?.messages.orEmpty()
    val record = loadConversationRecords().firstOrNull { it.conversationId == conversationId }
    val activeProjectName = activeBranch?.projectId?.let { loadChatProjectNames()[it] }
    val lastAssistantId = messages.lastOrNull { it.role == "assistant" }?.messageId
    val isArchived = record?.status == "archived"
    val canGenerate = readiness.inference.status == ReadinessStatus.READY && !isArchived
''',
)
replace_once(
    path,
    '''        activeBranch?.projectId?.let {
''',
    '''        record?.let { conversation ->
            Text(conversation.title, style = MaterialTheme.typography.headlineSmall)
            if (isArchived) {
                Text(
                    "Archived conversation · messages are read-only",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        activeBranch?.projectId?.let {
''',
)
replace_once(
    path,
    '''            enabled = activeOperationId == null,
''',
    '''            enabled = activeOperationId == null && !isArchived,
''',
)

# Replace branch/conversation parsers and summaries.
old = '''private fun summarizeChats(branches: List<ChatBranchCard>): List<ChatSummary> = branches
    .groupBy { it.conversationId }
    .mapNotNull { (conversationId, values) ->
        val latest = values.maxWithOrNull(
            compareBy<ChatBranchCard> { it.lastTimestamp }.thenBy { it.branchId },
        ) ?: return@mapNotNull null
        val firstUser = latest.messages.firstOrNull { it.role == "user" }?.content.orEmpty()
        val last = latest.messages.lastOrNull()?.content.orEmpty()
        ChatSummary(
            conversationId = conversationId,
            branchId = latest.branchId,
            title = firstUser.ifBlank { "Conversation" }.lineSequence().first().take(72),
            preview = last.ifBlank { firstUser }.replace('\\n', ' ').take(160),
            branchCount = values.size,
            projectId = latest.projectId,
            lastTimestamp = latest.lastTimestamp,
        )
    }
    .sortedByDescending { it.lastTimestamp }
'''
new = '''private fun loadConversationRecords(): List<ConversationRecordCard> {
    val raw = BackendRuntimeHost.requestRuntimeSnapshot("conversations") ?: return emptyList()
    return runCatching {
        val payload = JSONObject(raw).optJSONObject("payload") ?: JSONObject()
        val values = payload.optJSONArray("conversations") ?: JSONArray()
        buildList {
            for (index in 0 until values.length()) {
                val conversation = values.optJSONObject(index) ?: continue
                val conversationId = conversation.optString("conversation_id")
                val activeBranchId = conversation.optString("active_branch_id")
                if (conversationId.isBlank() || activeBranchId.isBlank()) continue
                add(
                    ConversationRecordCard(
                        conversationId = conversationId,
                        title = conversation.optString("title", "Conversation"),
                        projectId = conversation.optString("project_id").takeIf(String::isNotBlank),
                        activeBranchId = activeBranchId,
                        status = conversation.optString("status", "active"),
                        createdAt = conversation.optString("created_at"),
                        updatedAt = conversation.optString("updated_at"),
                    ),
                )
            }
        }
    }.getOrDefault(emptyList())
}

private fun summarizeChats(
    branches: List<ChatBranchCard>,
    conversations: List<ConversationRecordCard>,
): List<ChatSummary> {
    val branchesByConversation = branches.groupBy { it.conversationId }
    return conversations.map { conversation ->
        val values = branchesByConversation[conversation.conversationId].orEmpty()
        val active = values.firstOrNull { it.branchId == conversation.activeBranchId }
            ?: values.maxWithOrNull(compareBy<ChatBranchCard> { it.lastTimestamp }.thenBy { it.branchId })
        val firstUser = values
            .flatMap { it.messages }
            .filter { it.role == "user" }
            .minByOrNull { it.createdAt }
            ?.content
            .orEmpty()
        val last = active?.messages?.lastOrNull()?.content.orEmpty()
        ChatSummary(
            conversationId = conversation.conversationId,
            branchId = active?.branchId ?: conversation.activeBranchId,
            title = conversation.title,
            preview = last.ifBlank { firstUser }.replace('\\n', ' ').take(160),
            branchCount = values.size,
            projectId = conversation.projectId,
            status = conversation.status,
            lastTimestamp = conversation.updatedAt,
        )
    }.sortedByDescending { it.lastTimestamp }
}
'''
replace_once(path, old, new)
replace_once(
    path,
    '''private fun loadChatProjectNames(): Map<String, String> = loadActiveChatProjects()
    .associate { it.projectId to it.name }
''',
    '''private fun loadChatProjectNames(): Map<String, String> {
    val raw = BackendRuntimeHost.requestRuntimeSnapshot("projects") ?: return emptyMap()
    return runCatching {
        val payload = JSONObject(raw).optJSONObject("payload") ?: JSONObject()
        val projects = payload.optJSONArray("projects") ?: JSONArray()
        buildMap {
            for (index in 0 until projects.length()) {
                val project = projects.optJSONObject(index) ?: continue
                val projectId = project.optString("project_id")
                val name = project.optString("name")
                if (projectId.isNotBlank() && name.isNotBlank()) put(projectId, name)
            }
        }
    }.getOrDefault(emptyMap())
}
''',
)

print("actual conversation setup patch applied")
