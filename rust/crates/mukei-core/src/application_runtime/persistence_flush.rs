impl FeatureState {
    async fn flush_projections(&self) -> Result<(), MukeiError> {
        let store = self
            .projection_store
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(store) = store else { return Ok(()); };

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
        let conversations = self
            .conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|((conversation_id, branch_id), messages)| ConversationProjection {
                conversation_id: conversation_id.clone(),
                branch_id: branch_id.clone(),
                messages: messages.clone(),
            })
            .collect::<Vec<_>>();

        store.save("operations", serde_json::to_value(operations).map_err(|error| MukeiError::Internal(error.to_string()))?).await?;
        store.save("models", serde_json::to_value(models).map_err(|error| MukeiError::Internal(error.to_string()))?).await?;
        store.save("documents", serde_json::to_value(documents).map_err(|error| MukeiError::Internal(error.to_string()))?).await?;
        store.save("conversations", serde_json::to_value(conversations).map_err(|error| MukeiError::Internal(error.to_string()))?).await?;
        Ok(())
    }
}
