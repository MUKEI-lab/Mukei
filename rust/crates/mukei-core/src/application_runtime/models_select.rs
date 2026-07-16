impl MukeiRuntime {
    fn select_model(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(ack) = self.ensure_ready(command) {
            return ack;
        }
        let ValidatedCommandPayload::Model(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        let Some(descriptor) = crate::engine::lookup_model_str(&payload.model_id) else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        let Some(factory) = self.backend_factory.clone() else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::CapabilityUnavailable,
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
        if !path.is_file() {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::BackendUnavailable,
            );
        }
        let (acknowledgement, operation_id, token) = self.accept_operation(command);
        let activation = Arc::clone(&self.activation);
        let features = Arc::clone(&self.features);
        let events = Arc::clone(&self.events);
        let command_envelope = command.envelope.clone();
        let model_id = payload.model_id.clone();
        let expected_sha = descriptor.expected_sha256.to_owned();
        let operation_id_for_task = operation_id.clone();
        self.async_runtime.handle().spawn(async move {
            features.models.write().unwrap_or_else(|p| p.into_inner()).insert(
                model_id.clone(),
                ModelProjection {
                    model_id: model_id.clone(),
                    status: ModelStatus::Verifying,
                    local_path: Some(path.to_string_lossy().into_owned()),
                    progress: None,
                    error_code: None,
                },
            );
            let generation = activation.begin_verification(&model_id, &expected_sha);
            let verify_path = path.clone();
            let verify_sha = expected_sha.clone();
            let verification = tokio::task::spawn_blocking(move || {
                crate::storage::model_download::verify_file_sha256(&verify_path, &verify_sha)
            })
            .await;
            if token.is_cancelled() {
                features.update_operation(
                    &operation_id_for_task,
                    OperationStatus::Cancelled,
                    None,
                    Some("cancelled".into()),
                    Value::Null,
                );
                return;
            }
            match verification {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    activation.mark_verification_failed(
                        generation,
                        &model_id,
                        &expected_sha,
                        &expected_sha,
                        crate::engine::ActivationFailureCategory::VerificationMismatch,
                    );
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Failed,
                        None,
                        Some(error.error_code().into()),
                        Value::Null,
                    );
                    events.emit(
                        &format!("operation:{}", operation_id_for_task),
                        "operation.failed",
                        json!({"code": error.error_code()}),
                        Some(&command_envelope),
                        Some(operation_id_for_task),
                    );
                    return;
                }
                Err(error) => {
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Failed,
                        None,
                        Some("verification_join_failed".into()),
                        json!({"detail": error.to_string()}),
                    );
                    return;
                }
            }
            let artifact = match VerifiedModelArtifact::new(&expected_sha, path.clone()) {
                Ok(value) => value,
                Err(error) => {
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Failed,
                        None,
                        Some(error.error_code().into()),
                        Value::Null,
                    );
                    return;
                }
            };
            let verified = match VerifiedModelDescriptor::new(
                &model_id,
                &expected_sha,
                artifact,
            ) {
                Ok(value) => value,
                Err(error) => {
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Failed,
                        None,
                        Some(error.error_code().into()),
                        Value::Null,
                    );
                    return;
                }
            };
            if !activation.mark_verified(generation, verified) {
                features.update_operation(
                    &operation_id_for_task,
                    OperationStatus::Failed,
                    None,
                    Some("stale_activation".into()),
                    Value::Null,
                );
                return;
            }
            if let Some(model) = features.models.write().unwrap_or_else(|p| p.into_inner()).get_mut(&model_id) {
                model.status = ModelStatus::Activating;
            }
            match activation.activate_verified(factory.as_ref()).await {
                ActivationCommit::Ready => {
                    if let Some(model) = features.models.write().unwrap_or_else(|p| p.into_inner()).get_mut(&model_id) {
                        model.status = ModelStatus::Ready;
                    }
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Completed,
                        Some(1.0),
                        None,
                        json!({"model_id": model_id}),
                    );
                    events.emit(
                        "application:models",
                        "model.selected",
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
                other => {
                    if let Some(model) = features.models.write().unwrap_or_else(|p| p.into_inner()).get_mut(&model_id) {
                        model.status = ModelStatus::Failed;
                        model.error_code = Some(format!("activation_{other:?}"));
                    }
                    features.update_operation(
                        &operation_id_for_task,
                        OperationStatus::Failed,
                        None,
                        Some("activation_failed".into()),
                        Value::Null,
                    );
                    events.emit(
                        &format!("operation:{}", operation_id_for_task),
                        "operation.failed",
                        json!({"code": "activation_failed"}),
                        Some(&command_envelope),
                        Some(operation_id_for_task),
                    );
                }
            }
        });
        acknowledgement
    }

}
