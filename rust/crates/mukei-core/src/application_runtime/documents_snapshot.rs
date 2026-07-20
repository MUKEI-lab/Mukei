impl MukeiRuntime {
    fn activation_snapshot(&self) -> Value {
        let readiness = self.activation.readiness_snapshot();
        json!({
            "inference_interface_exists": readiness.inference_interface_exists,
            "real_backend_implementation_available": readiness.real_backend_implementation_available,
            "selected_model_exists": readiness.selected_model_exists,
            "selected_model_verified": readiness.selected_model_verified,
            "activation_in_progress": readiness.activation_in_progress,
            "active_backend_ready": readiness.active_backend_ready,
            "development_mock_active": readiness.development_mock_active,
            "activation_failed": readiness.activation_failed,
            "product_ready": readiness.product_ready,
            "state": format!("{:?}", readiness.state),
        })
    }

    pub fn snapshot(
        &self,
        domain: RuntimeSnapshotDomain,
    ) -> Result<RuntimeSnapshotEnvelope, RuntimeError> {
        if self.closed.load(Ordering::Acquire) && domain != RuntimeSnapshotDomain::Application {
            return Err(RuntimeError::Stopped);
        }
        let payload = match domain {
            RuntimeSnapshotDomain::Application => json!({
                "state": self.state(),
                "runtime_session_id": self.session_id,
                "app_data_dir": self.config.app_data_dir,
                "cancelled": self.cancellation.is_cancelled(),
                "inference": self.activation_snapshot(),
                "platform": self.platform.snapshot(),
            }),
            RuntimeSnapshotDomain::Settings => json!({
                "values": self.settings.read().unwrap_or_else(|p| p.into_inner()).clone(),
            }),
            RuntimeSnapshotDomain::Protocol => serde_json::to_value(self.capabilities())
                .map_err(|_| RuntimeError::UnsupportedSnapshot)?,
            RuntimeSnapshotDomain::Operations => {
                let mut snapshot = self.features.snapshot_with_conversations(self.platform.snapshot());
                #[cfg(feature = "rusqlite")]
                if let Some(object) = snapshot.as_object_mut() {
                    let attachments = match self
                        .conversation_attachments
                        .read()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone()
                    {
                        Some(service) => self
                            .async_runtime
                            .block_on(service.list_all())
                            .map_err(|_| RuntimeError::UnsupportedSnapshot)?,
                        None => Vec::new(),
                    };
                    object.insert(
                        "conversation_attachments".to_owned(),
                        serde_json::to_value(attachments)
                            .map_err(|_| RuntimeError::UnsupportedSnapshot)?,
                    );
                }
                snapshot
            }
            RuntimeSnapshotDomain::Projects => self.features.projects_snapshot(),
            RuntimeSnapshotDomain::Storage => {
                #[cfg(feature = "rusqlite")]
                {
                    let service = self
                        .storage_workspace
                        .read()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone()
                        .ok_or(RuntimeError::UnsupportedSnapshot)?;
                    let snapshot = self
                        .async_runtime
                        .block_on(service.universal_snapshot())
                        .map_err(|_| RuntimeError::UnsupportedSnapshot)?;
                    serde_json::to_value(snapshot)
                        .map_err(|_| RuntimeError::UnsupportedSnapshot)?
                }
                #[cfg(not(feature = "rusqlite"))]
                {
                    return Err(RuntimeError::UnsupportedSnapshot);
                }
            }
        };
        Ok(RuntimeSnapshotEnvelope {
            runtime_session_id: self.session_id.clone(),
            domain,
            schema_version: 2,
            generated_at: Utc::now(),
            payload,
        })
    }

    /// Begin deterministic shutdown. Repeated calls are idempotent.
    pub fn shutdown(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        *self.state.write().unwrap_or_else(|p| p.into_inner()) = RuntimeState::Stopping;
        self.events.emit(
            "application:lifecycle",
            "runtime.stopping",
            json!({"runtime_session_id": self.session_id}),
            None,
            None,
        );
        self.cancellation.cancel();
        self.features.cancel_all();
        self.activation.deactivate();
        if let Err(error) = self.async_runtime.block_on(self.features.flush_projections()) {
            tracing::error!(
                code = error.error_code(),
                "encrypted projection flush failed during shutdown"
            );
        }
        if let Err(error) = self.persist_settings_now() {
            tracing::error!(
                code = error.error_code(),
                "encrypted settings flush failed during shutdown"
            );
        }
        *self.state.write().unwrap_or_else(|p| p.into_inner()) = RuntimeState::Stopped;
        self.events.emit(
            "application:lifecycle",
            "runtime.stopped",
            json!({"runtime_session_id": self.session_id}),
            None,
            None,
        );
    }
}
