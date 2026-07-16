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
    Indexed,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ConversationProjection {
    conversation_id: String,
    branch_id: String,
    messages: Vec<ChatMessage>,
}

enum PersistenceCommand {
    Save {
        store: Arc<dyn RuntimeProjectionStore>,
        key: &'static str,
        value: Value,
    },
    Barrier(tokio::sync::oneshot::Sender<()>),
}

struct FeatureState {
    operations: RwLock<HashMap<String, OperationRecord>>,
    operation_tokens: Mutex<HashMap<String, CancellationToken>>,
    conversations: RwLock<HashMap<(String, String), Vec<ChatMessage>>>,
    models: RwLock<HashMap<String, ModelProjection>>,
    documents: RwLock<HashMap<String, DocumentProjection>>,
    projection_store: RwLock<Option<Arc<dyn RuntimeProjectionStore>>>,
    persistence_sender: mpsc::UnboundedSender<PersistenceCommand>,
    persistence_enqueue: Mutex<()>,
}

impl FeatureState {
    fn new(runtime_handle: tokio::runtime::Handle) -> Self {
        let (persistence_sender, mut persistence_receiver) =
            mpsc::unbounded_channel::<PersistenceCommand>();
        runtime_handle.spawn(async move {
            while let Some(command) = persistence_receiver.recv().await {
                match command {
                    PersistenceCommand::Save { store, key, value } => {
                        if let Err(error) = store.save(key, value).await {
                            tracing::error!(
                                code = error.error_code(),
                                projection = key,
                                "projection save failed"
                            );
                        }
                    }
                    PersistenceCommand::Barrier(acknowledgement) => {
                        let _ = acknowledgement.send(());
                    }
                }
            }
        });

        Self {
            operations: RwLock::new(HashMap::new()),
            operation_tokens: Mutex::new(HashMap::new()),
            conversations: RwLock::new(HashMap::new()),
            models: RwLock::new(HashMap::new()),
            documents: RwLock::new(HashMap::new()),
            projection_store: RwLock::new(None),
            persistence_sender,
            persistence_enqueue: Mutex::new(()),
        }
    }

    fn attach_projection_store(&self, store: Arc<dyn RuntimeProjectionStore>) {
        *self
            .projection_store
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(store);
    }

    async fn hydrate_from_store(&self) -> Result<(), MukeiError> {
        let store = self
            .projection_store
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(store) = store else { return Ok(()); };

        if let Some(value) = store.load("operations").await? {
            let mut records: Vec<OperationRecord> =
                serde_json::from_value(value).map_err(|_| MukeiError::DatabaseCorruption)?;
            for record in &mut records {
                if matches!(
                    record.status,
                    OperationStatus::Accepted | OperationStatus::Running
                ) {
                    record.status = OperationStatus::Failed;
                    record.updated_at = Utc::now();
                    record.detail = Some("interrupted_by_process_death".into());
                }
            }
            *self
                .operations
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = records
                .into_iter()
                .map(|record| (record.operation_id.clone(), record))
                .collect();
        }
        if let Some(value) = store.load("models").await? {
            let records: Vec<ModelProjection> =
                serde_json::from_value(value).map_err(|_| MukeiError::DatabaseCorruption)?;
            *self.models.write().unwrap_or_else(|p| p.into_inner()) = records
                .into_iter()
                .map(|record| (record.model_id.clone(), record))
                .collect();
        }
        if let Some(value) = store.load("documents").await? {
            let records: Vec<DocumentProjection> =
                serde_json::from_value(value).map_err(|_| MukeiError::DatabaseCorruption)?;
            *self.documents.write().unwrap_or_else(|p| p.into_inner()) = records
                .into_iter()
                .map(|record| (record.document_id.clone(), record))
                .collect();
        }
        if let Some(value) = store.load("conversations").await? {
            let records: Vec<ConversationProjection> =
                serde_json::from_value(value).map_err(|_| MukeiError::DatabaseCorruption)?;
            *self
                .conversations
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = records
                .into_iter()
                .map(|record| {
                    (
                        (record.conversation_id, record.branch_id),
                        record.messages,
                    )
                })
                .collect();
        }
        self.persist_operations();
        Ok(())
    }

    fn persist_value(&self, key: &'static str, value: Value) {
        let store = self
            .projection_store
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(store) = store else { return; };
        if self
            .persistence_sender
            .send(PersistenceCommand::Save { store, key, value })
            .is_err()
        {
            tracing::error!(projection = key, "projection writer unavailable");
        }
    }

    fn persist_operations(&self) {
        let _enqueue = self
            .persistence_enqueue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let records = self
            .operations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if let Ok(value) = serde_json::to_value(records) {
            self.persist_value("operations", value);
        }
    }

    fn persist_models(&self) {
        let _enqueue = self
            .persistence_enqueue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let records = self
            .models
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if let Ok(value) = serde_json::to_value(records) {
            self.persist_value("models", value);
        }
    }

    fn persist_documents(&self) {
        let _enqueue = self
            .persistence_enqueue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let records = self
            .documents
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if let Ok(value) = serde_json::to_value(records) {
            self.persist_value("documents", value);
        }
    }

    fn persist_conversations(&self) {
        let _enqueue = self
            .persistence_enqueue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let records = self
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
        if let Ok(value) = serde_json::to_value(records) {
            self.persist_value("conversations", value);
        }
    }

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
        self.persist_operations();
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
        self.persist_operations();
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
        self.persist_conversations();
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
        let Some(messages) =
            conversations.get_mut(&(conversation.to_owned(), branch.to_owned()))
        else {
            return false;
        };
        let Some(index) = messages
            .iter()
            .rposition(|message| message.role == Role::Assistant)
        else {
            return false;
        };
        messages.remove(index);
        drop(conversations);
        self.persist_conversations();
        true
    }

    fn insert_model(&self, model: ModelProjection) {
        self.models
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(model.model_id.clone(), model);
        self.persist_models();
    }

    fn update_model(&self, model_id: &str, update: impl FnOnce(&mut ModelProjection)) -> bool {
        let mut models = self.models.write().unwrap_or_else(|p| p.into_inner());
        let Some(model) = models.get_mut(model_id) else {
            return false;
        };
        update(model);
        drop(models);
        self.persist_models();
        true
    }

    fn remove_model(&self, model_id: &str) -> Option<ModelProjection> {
        let removed = self
            .models
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(model_id);
        self.persist_models();
        removed
    }

    fn model(&self, model_id: &str) -> Option<ModelProjection> {
        self.models
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(model_id)
            .cloned()
    }

    fn insert_document(&self, document: DocumentProjection) {
        self.documents
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(document.document_id.clone(), document);
        self.persist_documents();
    }

    fn update_document(
        &self,
        document_id: &str,
        update: impl FnOnce(&mut DocumentProjection),
    ) -> bool {
        let mut documents = self.documents.write().unwrap_or_else(|p| p.into_inner());
        let Some(document) = documents.get_mut(document_id) else {
            return false;
        };
        update(document);
        drop(documents);
        self.persist_documents();
        true
    }

    fn remove_document(&self, document_id: &str) -> Option<DocumentProjection> {
        let removed = self
            .documents
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(document_id);
        self.persist_documents();
        removed
    }

    fn document(&self, document_id: &str) -> Option<DocumentProjection> {
        self.documents
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(document_id)
            .cloned()
    }

    fn active_operation_ids(&self, command_type: &str) -> Vec<String> {
        self.operations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .filter(|record| {
                record.command_type == command_type
                    && matches!(
                        record.status,
                        OperationStatus::Accepted | OperationStatus::Running
                    )
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
