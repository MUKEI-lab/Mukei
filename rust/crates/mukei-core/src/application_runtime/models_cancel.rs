impl MukeiRuntime {
    fn cancel_download(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Some(operation_id) = command.envelope.operation_id.as_deref() {
            if !self.features.cancel_operation(operation_id) {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::StaleScope,
                );
            }
            self.events.emit(
                &format!("operation:{operation_id}"),
                "operation.cancel_requested",
                json!({"state": "cancel_requested", "kind": "model_download"}),
                Some(&command.envelope),
                Some(operation_id.to_owned()),
            );
            return CommandAcknowledgementV2::accepted(
                &command.envelope,
                Some(operation_id.to_owned()),
            );
        }
        let targets = self.features.active_operation_ids(CommandType::ModelDownload.as_str());
        if targets.is_empty() {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            );
        }
        for operation_id in &targets {
            self.features.cancel_operation(operation_id);
            self.events.emit(
                &format!("operation:{operation_id}"),
                "operation.cancel_requested",
                json!({"state": "cancel_requested", "kind": "model_download"}),
                Some(&command.envelope),
                Some(operation_id.clone()),
            );
        }
        CommandAcknowledgementV2::accepted(&command.envelope, targets.first().cloned())
    }

}
