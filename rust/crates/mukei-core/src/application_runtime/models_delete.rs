impl MukeiRuntime {
    fn delete_model(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(ack) = self.ensure_ready(command) {
            return ack;
        }
        let ValidatedCommandPayload::Model(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        if self
            .activation
            .active_model_snapshot()
            .is_some_and(|active| active.model_id == payload.model_id)
        {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::BusyConflict,
            );
        }
        let Some(descriptor) = crate::engine::lookup_model_str(&payload.model_id) else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        let Some(config) = self
            .product_config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::BackendUnavailable,
            );
        };
        let path = config.models_dir.join(descriptor.filename);
        let (acknowledgement, operation_id, token) = self.accept_operation(command);
        let model_id = payload.model_id.clone();
        let features = Arc::clone(&self.features);
        let events = Arc::clone(&self.events);
        let command_envelope = command.envelope.clone();
        let operation_id_for_task = operation_id.clone();
        self.async_runtime.handle().spawn(async move {
            if token.is_cancelled() {
                return;
            }
            let result = tokio::task::spawn_blocking(move || {
                if path.exists() {
                    std::fs::remove_file(path)
                } else {
                    Ok(())
                }
            })
            .await;
            match result {
                Ok(Ok(())) => {
                    features
                        .models
                        .write()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .remove(&model_id);
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Completed,
                        Some(1.0),
                        None,
                        json!({"model_id": model_id}),
                    );
                    events.emit(
                        "application:models",
                        "model.deleted",
                        json!({"model_id": model_id}),
                        Some(&command_envelope),
                        Some(operation_id_for_task.clone()),
                    );
                    events.emit(
                        &format!("operation:{}", operation_id_for_task),
                        "operation.completed",
                        json!({"state": "completed"}),
                        Some(&command_envelope),
                        Some(operation_id_for_task),
                    );
                }
                Ok(Err(error)) => {
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Failed,
                        None,
                        Some("model_delete_failed".into()),
                        json!({"detail": error.to_string()}),
                    );
                    events.emit(
                        &format!("operation:{}", operation_id_for_task),
                        "operation.failed",
                        json!({"code": "model_delete_failed"}),
                        Some(&command_envelope),
                        Some(operation_id_for_task),
                    );
                }
                Err(error) => {
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Failed,
                        None,
                        Some("model_delete_join_failed".into()),
                        json!({"detail": error.to_string()}),
                    );
                }
            }
        });
        acknowledgement
    }

}
