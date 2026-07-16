#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OperationStatus {
    Accepted,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct OperationRecord {
    operation_id: String,
    command_type: String,
    status: OperationStatus,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    scope: Option<crate::ui_protocol::CommandScope>,
    progress: Option<f64>,
    detail: Option<String>,
    result: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ModelStatus {
    Downloading,
    Installed,
    Verifying,
    Activating,
    Ready,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ModelProjection {
    model_id: String,
    status: ModelStatus,
    local_path: Option<String>,
    progress: Option<f64>,
    error_code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DocumentStatus {
    Staging,
    Staged,
    IngestionUnavailable,
    Revoked,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DocumentProjection {
    document_id: String,
    label: String,
    mime_type: String,
    source_fingerprint: String,
    staged_path: Option<String>,
    size_bytes: Option<u64>,
    status: DocumentStatus,
    error_code: Option<String>,
}

#[derive(Default)]
struct FeatureState {
    operations: RwLock<HashMap<String, OperationRecord>>,
    operation_tokens: Mutex<HashMap<String, CancellationToken>>,
    conversations: RwLock<HashMap<(String, String), Vec<ChatMessage>>>,
    models: RwLock<HashMap<String, ModelProjection>>,
    documents: RwLock<HashMap<String, DocumentProjection>>,
}

impl FeatureState {
    fn create_operation(&self, command: &ValidatedCommand) -> (String, CancellationToken) {
        let operation_id = command
            .envelope
            .operation_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let token = CancellationToken::new();
        self.operations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                operation_id.clone(),
                OperationRecord {
                    operation_id: operation_id.clone(),
                    command_type: command.envelope.command_type.clone(),
                    status: OperationStatus::Accepted,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    scope: command.envelope.scope.clone(),
                    progress: None,
                    detail: None,
                    result: Value::Null,
                },
            );
        self.operation_tokens
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(operation_id.clone(), token.clone());
        (operation_id, token)
    }

    fn update_operation(
        &self,
        operation_id: &str,
        status: OperationStatus,
        progress: Option<f64>,
        detail: Option<String>,
        result: Value,
    ) {
        if let Some(record) = self
            .operations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get_mut(operation_id)
        {
            record.status = status;
            record.updated_at = Utc::now();
            record.progress = progress;
            record.detail = detail;
            record.result = result;
        }
        if matches!(
            status,
            OperationStatus::Completed | OperationStatus::Failed | OperationStatus::Cancelled
        ) {
            self.operation_tokens
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(operation_id);
        }
    }

    fn cancel_operation(&self, operation_id: &str) -> bool {
        let token = self
            .operation_tokens
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(operation_id)
            .cloned();
        if let Some(token) = token {
            token.cancel();
            self.update_operation(
                operation_id,
                OperationStatus::Cancelled,
                None,
                Some("cancel_requested".into()),
                Value::Null,
            );
            true
        } else {
            false
        }
    }

    fn cancel_all(&self) {
        let tokens = self
            .operation_tokens
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for token in tokens {
            token.cancel();
        }
    }

    fn append_message(&self, conversation: &str, branch: &str, message: ChatMessage) {
        self.conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry((conversation.to_owned(), branch.to_owned()))
            .or_default()
            .push(message);
    }

    fn history(&self, conversation: ConversationId, branch: BranchId) -> Vec<ChatMessage> {
        self.conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(conversation.0.to_string(), branch.0.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    fn clear_conversation(&self, conversation: &str, branch: &str) -> usize {
        self.conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&(conversation.to_owned(), branch.to_owned()))
            .map(|messages| messages.len())
            .unwrap_or(0)
    }

    fn last_user_message(&self, conversation: &str, branch: &str) -> Option<ChatMessage> {
        self.conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(conversation.to_owned(), branch.to_owned()))
            .and_then(|messages| {
                messages
                    .iter()
                    .rev()
                    .find(|message| message.role == Role::User)
                    .cloned()
            })
    }

    fn remove_last_assistant(&self, conversation: &str, branch: &str) -> bool {
        let mut conversations = self
            .conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(messages) = conversations.get_mut(&(conversation.to_owned(), branch.to_owned())) else {
            return false;
        };
        let Some(index) = messages.iter().rposition(|message| message.role == Role::Assistant) else {
            return false;
        };
        messages.remove(index);
        true
    }

    fn active_operation_ids(&self, command_type: &str) -> Vec<String> {
        self.operations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .filter(|record| {
                record.command_type == command_type
                    && matches!(record.status, OperationStatus::Accepted | OperationStatus::Running)
            })
            .map(|record| record.operation_id.clone())
            .collect()
    }

    fn snapshot(&self, platform: PlatformBrokerSnapshot) -> Value {
        let operations = self
            .operations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let models = self
            .models
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let documents = self
            .documents
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let conversation_count = self
            .conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();
        json!({
            "operations": operations,
            "models": models,
            "documents": documents,
            "conversation_count": conversation_count,
            "platform": platform,
        })
    }
}
