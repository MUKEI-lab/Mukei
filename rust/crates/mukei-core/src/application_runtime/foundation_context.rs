struct RuntimeContextBackend {
    features: Arc<FeatureState>,
    ephemeral_chats: Arc<EphemeralChatState>,
    rag_service: Option<Arc<dyn RuntimeRagService>>,
}

#[async_trait::async_trait]
impl ContextBackend for RuntimeContextBackend {
    async fn load_history(
        &self,
        conversation: ConversationId,
        branch: BranchId,
        active_history: &[ChatMessage],
    ) -> Result<Vec<ChatMessage>, MukeiError> {
        let active_ids = active_history.iter().map(|value| value.id).collect::<Vec<_>>();
        let history = self
            .ephemeral_chats
            .history_if_registered(conversation, branch)
            .unwrap_or_else(|| self.features.history(conversation, branch));
        Ok(history
            .into_iter()
            .filter(|message| !active_ids.contains(&message.id))
            .collect())
    }

    async fn rag_lookup(&self, query: &str, top_k: usize) -> Result<Vec<String>, MukeiError> {
        let Some(service) = self.rag_service.as_ref() else {
            return Ok(Vec::new());
        };
        let values = service.retrieve(query, top_k).await?;
        Ok(values
            .into_iter()
            .map(|value| {
                crate::tools::sentinel::wrap_external_data(
                    crate::tools::sentinel::ExternalDataSource::Rag,
                    &value,
                )
            })
            .collect())
    }
}

struct RuntimeTokenCounter;

#[async_trait::async_trait]
impl TokenCount for RuntimeTokenCounter {
    async fn count(&self, value: &str) -> usize {
        value.len().div_ceil(4)
    }
}

struct CommandRouter;

impl CommandRouter {
    fn dispatch(
        runtime: &MukeiRuntime,
        command: &ValidatedCommand,
    ) -> CommandAcknowledgementV2 {
        match command.command_type {
            CommandType::AppInitialize => runtime.initialize(command),
            CommandType::ChatSendMessage => runtime.send_message(command),
            CommandType::ChatStopGeneration => runtime.stop_generation(command),
            CommandType::ChatClearConversation => runtime.clear_conversation(command),
            CommandType::ModelDownload => runtime.download_model(command),
            CommandType::DownloadCancel => runtime.cancel_download(command),
            CommandType::ModelSelect => runtime.select_model(command),
            CommandType::ModelDelete => runtime.delete_model(command),
            CommandType::DocumentGrant => runtime.grant_document(command),
            CommandType::DocumentRevoke => runtime.revoke_document(command),
            CommandType::DocumentRetryIngestion => runtime.retry_document_ingestion(command),
            CommandType::SettingsUpdate => runtime.update_setting(command),
            CommandType::RecoveryResume => runtime.recover_chat(command, false),
            CommandType::RecoveryRegenerate => runtime.recover_chat(command, true),
        }
    }
}
