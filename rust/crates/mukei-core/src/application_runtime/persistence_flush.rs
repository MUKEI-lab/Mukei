impl FeatureState {
    async fn flush_projections(&self) -> Result<(), MukeiError> {
        let acknowledgement = {
            let _enqueue = self
                .persistence_enqueue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let store = self
                .projection_store
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            let Some(store) = store else {
                return Ok(());
            };
            let operations = self
                .operations
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .values()
                .cloned()
                .collect::<Vec<_>>();
            let models = self
                .models
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .values()
                .cloned()
                .collect::<Vec<_>>();
            let documents = self
                .documents
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .values()
                .cloned()
                .collect::<Vec<_>>();
            let projects = self
                .projects
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .values()
                .cloned()
                .collect::<Vec<_>>();
            let conversations = self
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
            let projections = vec![
                (
                    "operations",
                    serde_json::to_value(operations)
                        .map_err(|error| MukeiError::Internal(error.to_string()))?,
                ),
                (
                    "models",
                    serde_json::to_value(models)
                        .map_err(|error| MukeiError::Internal(error.to_string()))?,
                ),
                (
                    "documents",
                    serde_json::to_value(documents)
                        .map_err(|error| MukeiError::Internal(error.to_string()))?,
                ),
                (
                    "conversations",
                    serde_json::to_value(conversations)
                        .map_err(|error| MukeiError::Internal(error.to_string()))?,
                ),
                (
                    "projects",
                    serde_json::to_value(projects)
                        .map_err(|error| MukeiError::Internal(error.to_string()))?,
                ),
            ];
            let (sender, receiver) = tokio::sync::oneshot::channel();
            self.persistence_sender
                .send(PersistenceCommand::Flush {
                    store,
                    projections,
                    acknowledgement: sender,
                })
                .map_err(|_| MukeiError::Internal("projection writer unavailable".into()))?;
            receiver
        };
        acknowledgement
            .await
            .map_err(|_| MukeiError::Internal("projection writer stopped before flush".into()))??;
        Ok(())
    }
}
