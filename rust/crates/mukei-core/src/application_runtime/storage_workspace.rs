#[cfg(feature = "rusqlite")]
impl MukeiRuntime {
    fn storage_workspace_port(
        &self,
        command: &ValidatedCommand,
    ) -> Result<Arc<dyn crate::storage::StorageWorkspacePort>, CommandAcknowledgementV2> {
        self.storage_workspace
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(|| {
                CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::CapabilityUnavailable,
                )
            })
    }

    fn create_storage_directory(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(acknowledgement) = self.ensure_ready(command) {
            return acknowledgement;
        }
        let ValidatedCommandPayload::StorageDirectoryCreate(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        let service = match self.storage_workspace_port(command) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        let parent_node_id = match parse_storage_node_id(&payload.parent_node_id) {
            Some(value) => value,
            None => {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::StaleScope,
                )
            }
        };
        let display_name = payload.name.clone();
        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        let features = Arc::clone(&self.features);
        let events = Arc::clone(&self.events);
        let envelope = command.envelope.clone();
        let operation_id_for_task = operation_id.clone();
        self.async_runtime.handle().spawn(async move {
            match service.create_directory(parent_node_id, display_name).await {
                Ok(node) => complete_storage_workspace_operation(
                    &features,
                    &events,
                    &envelope,
                    &operation_id_for_task,
                    "storage.directory.created",
                    serde_json::to_value(node).unwrap_or(Value::Null),
                ),
                Err(_) => fail_storage_workspace_operation(
                    &features,
                    &events,
                    &envelope,
                    &operation_id_for_task,
                    "storage_directory_create_failed",
                ),
            }
        });
        acknowledgement
    }

    fn rename_storage_node(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(acknowledgement) = self.ensure_ready(command) {
            return acknowledgement;
        }
        let ValidatedCommandPayload::StorageNodeRename(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        let service = match self.storage_workspace_port(command) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        let node_id = match parse_storage_node_id(&payload.node_id) {
            Some(value) => value,
            None => {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::StaleScope,
                )
            }
        };
        let display_name = payload.name.clone();
        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        let features = Arc::clone(&self.features);
        let events = Arc::clone(&self.events);
        let envelope = command.envelope.clone();
        let operation_id_for_task = operation_id.clone();
        self.async_runtime.handle().spawn(async move {
            match service.rename_node(node_id, display_name).await {
                Ok(node) => complete_storage_workspace_operation(
                    &features,
                    &events,
                    &envelope,
                    &operation_id_for_task,
                    "storage.node.renamed",
                    serde_json::to_value(node).unwrap_or(Value::Null),
                ),
                Err(_) => fail_storage_workspace_operation(
                    &features,
                    &events,
                    &envelope,
                    &operation_id_for_task,
                    "storage_node_rename_failed",
                ),
            }
        });
        acknowledgement
    }

    fn trash_storage_node(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        self.mutate_storage_node(command, true)
    }

    fn restore_storage_node(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        self.mutate_storage_node(command, false)
    }

    fn mutate_storage_node(
        &self,
        command: &ValidatedCommand,
        trash: bool,
    ) -> CommandAcknowledgementV2 {
        if let Err(acknowledgement) = self.ensure_ready(command) {
            return acknowledgement;
        }
        let ValidatedCommandPayload::StorageNode(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        let service = match self.storage_workspace_port(command) {
            Ok(value) => value,
            Err(acknowledgement) => return acknowledgement,
        };
        let node_id = match parse_storage_node_id(&payload.node_id) {
            Some(value) => value,
            None => {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::StaleScope,
                )
            }
        };
        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        let features = Arc::clone(&self.features);
        let events = Arc::clone(&self.events);
        let envelope = command.envelope.clone();
        let operation_id_for_task = operation_id.clone();
        self.async_runtime.handle().spawn(async move {
            let result = if trash {
                service.trash_node(node_id).await
            } else {
                service.restore_node(node_id).await
            };
            match result {
                Ok(node) => complete_storage_workspace_operation(
                    &features,
                    &events,
                    &envelope,
                    &operation_id_for_task,
                    if trash {
                        "storage.node.trashed"
                    } else {
                        "storage.node.restored"
                    },
                    serde_json::to_value(node).unwrap_or(Value::Null),
                ),
                Err(_) => fail_storage_workspace_operation(
                    &features,
                    &events,
                    &envelope,
                    &operation_id_for_task,
                    if trash {
                        "storage_node_trash_failed"
                    } else {
                        "storage_node_restore_failed"
                    },
                ),
            }
        });
        acknowledgement
    }
}

#[cfg(feature = "rusqlite")]
fn parse_storage_node_id(value: &str) -> Option<crate::storage::StorageNodeId> {
    Uuid::parse_str(value).ok().map(crate::storage::StorageNodeId)
}

#[cfg(feature = "rusqlite")]
fn complete_storage_workspace_operation(
    features: &FeatureState,
    events: &EventBus,
    command: &CommandEnvelopeV2,
    operation_id: &str,
    event_type: &str,
    result: Value,
) {
    features.update_operation(
        operation_id,
        OperationStatus::Completed,
        Some(1.0),
        None,
        result.clone(),
    );
    events.emit(
        "application:storage",
        event_type,
        result,
        Some(command),
        Some(operation_id.to_owned()),
    );
    events.emit(
        &format!("operation:{operation_id}"),
        "operation.completed",
        json!({"state": "completed"}),
        Some(command),
        Some(operation_id.to_owned()),
    );
}

#[cfg(feature = "rusqlite")]
fn fail_storage_workspace_operation(
    features: &FeatureState,
    events: &EventBus,
    command: &CommandEnvelopeV2,
    operation_id: &str,
    code: &str,
) {
    features.update_operation(
        operation_id,
        OperationStatus::Failed,
        None,
        Some(code.to_owned()),
        Value::Null,
    );
    events.emit(
        &format!("operation:{operation_id}"),
        "operation.failed",
        json!({"code": code}),
        Some(command),
        Some(operation_id.to_owned()),
    );
}

#[cfg(not(feature = "rusqlite"))]
impl MukeiRuntime {
    fn create_storage_directory(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        CommandAcknowledgementV2::rejected(
            Some(&command.envelope),
            RejectionReason::CapabilityUnavailable,
        )
    }

    fn rename_storage_node(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        self.create_storage_directory(command)
    }

    fn trash_storage_node(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        self.create_storage_directory(command)
    }

    fn restore_storage_node(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        self.create_storage_directory(command)
    }
}
