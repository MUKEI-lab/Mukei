impl MukeiRuntime {
    fn download_model(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(ack) = self.ensure_ready(command) {
            return ack;
        }
        #[cfg(not(feature = "network"))]
        {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::CapabilityUnavailable,
            );
        }
        #[cfg(feature = "network")]
        {
            let ValidatedCommandPayload::ModelDownload(payload) = &command.payload else {
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
            if !payload.sha256.is_empty() && payload.sha256 != descriptor.expected_sha256 {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::PolicyDenied,
                );
            }
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
            let destination = config.models_dir.join(descriptor.filename);
            let (acknowledgement, operation_id, token) = self.accept_operation(command);
            self.features
                .models
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(
                    payload.model_id.clone(),
                    ModelProjection {
                        model_id: payload.model_id.clone(),
                        status: ModelStatus::Downloading,
                        local_path: None,
                        progress: Some(0.0),
                        error_code: None,
                    },
                );
            let features = Arc::clone(&self.features);
            let events = Arc::clone(&self.events);
            let command_envelope = command.envelope.clone();
            let model_id = payload.model_id.clone();
            let request = crate::storage::model_download::DownloadRequest {
                url: descriptor.download_url.to_owned(),
                expected_sha256: descriptor.expected_sha256.to_owned(),
                dest: destination,
            };
            let operation_id_for_task = operation_id.clone();
            self.async_runtime.handle().spawn(async move {
                let (sender, mut receiver) = mpsc::channel(32);
                let download = crate::storage::model_download::run_download(request, sender, token);
                tokio::pin!(download);
                let result = loop {
                    tokio::select! {
                        result = &mut download => break result,
                        event = receiver.recv() => {
                            let Some(event) = event else { continue; };
                            match event {
                                crate::storage::model_download::DownloadEvent::Started { total_bytes } => {
                                    events.emit(
                                        &format!("operation:{}", operation_id_for_task),
                                        "model.download.started",
                                        json!({"model_id": model_id, "total_bytes": total_bytes}),
                                        Some(&command_envelope),
                                        Some(operation_id_for_task.clone()),
                                    );
                                }
                                crate::storage::model_download::DownloadEvent::Progress { progress, bytes_downloaded } => {
                                    features.update_operation(
                                        &operation_id_for_task,
                                        OperationStatus::Running,
                                        Some(progress),
                                        None,
                                        json!({"bytes_downloaded": bytes_downloaded}),
                                    );
                                    if let Some(model) = features.models.write().unwrap_or_else(|p| p.into_inner()).get_mut(&model_id) {
                                        model.progress = Some(progress);
                                    }
                                    events.emit(
                                        &format!("operation:{}", operation_id_for_task),
                                        "model.download.progress",
                                        json!({"model_id": model_id, "progress": progress, "bytes_downloaded": bytes_downloaded}),
                                        Some(&command_envelope),
                                        Some(operation_id_for_task.clone()),
                                    );
                                }
                                crate::storage::model_download::DownloadEvent::Complete { final_path } => {
                                    if let Some(model) = features.models.write().unwrap_or_else(|p| p.into_inner()).get_mut(&model_id) {
                                        model.status = ModelStatus::Installed;
                                        model.local_path = Some(final_path.to_string_lossy().into_owned());
                                        model.progress = Some(1.0);
                                    }
                                }
                                crate::storage::model_download::DownloadEvent::Error { code, .. } => {
                                    if let Some(model) = features.models.write().unwrap_or_else(|p| p.into_inner()).get_mut(&model_id) {
                                        model.status = ModelStatus::Failed;
                                        model.error_code = Some(code.to_owned());
                                    }
                                }
                            }
                        }
                    }
                };
                match result {
                    Ok(()) => {
                        features.update_operation(
                            &operation_id_for_task,
                            OperationStatus::Completed,
                            Some(1.0),
                            None,
                            json!({"model_id": model_id}),
                        );
                        events.emit(
                            &format!("operation:{}", operation_id_for_task),
                            "operation.completed",
                            json!({"model_id": model_id}),
                            Some(&command_envelope),
                            Some(operation_id_for_task),
                        );
                    }
                    Err(error) => {
                        features.update_operation(
                            &operation_id_for_task,
                            if matches!(error, MukeiError::Cancelled) { OperationStatus::Cancelled } else { OperationStatus::Failed },
                            None,
                            Some(error.error_code().into()),
                            Value::Null,
                        );
                        events.emit(
                            &format!("operation:{}", operation_id_for_task),
                            "operation.failed",
                            json!({"code": error.error_code(), "model_id": model_id}),
                            Some(&command_envelope),
                            Some(operation_id_for_task),
                        );
                    }
                }
            });
            acknowledgement
        }
    }

}
