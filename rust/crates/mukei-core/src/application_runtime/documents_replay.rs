impl MukeiRuntime {
    fn fingerprint(command: &ValidatedCommand) -> Option<Vec<u8>> {
        serde_json::to_vec(&json!({
            "command_type": command.envelope.command_type,
            "operation_id": command.envelope.operation_id,
            "scope": command.envelope.scope,
            "payload": command.envelope.payload,
        }))
        .ok()
    }

    fn replay_lookup(&self, command: &ValidatedCommand) -> Option<CommandAcknowledgementV2> {
        let key = command.envelope.idempotency_key.as_ref()?;
        let fingerprint = Self::fingerprint(command)?;
        let replay = self.replay.lock().unwrap_or_else(|p| p.into_inner());
        replay.get(key).map(|record| {
            if record.fingerprint != fingerprint {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::DuplicateReplayConflict,
                );
            }
            let mut acknowledgement = record.acknowledgement.clone();
            acknowledgement.command_id = command.envelope.command_id.clone();
            acknowledgement.request_id = command.envelope.request_id.clone();
            acknowledgement.correlation_id = command.envelope.correlation_id.clone();
            acknowledgement.timestamp = Utc::now();
            acknowledgement
        })
    }

    fn remember_replay(&self, command: &ValidatedCommand, acknowledgement: &CommandAcknowledgementV2) {
        let Some(key) = command.envelope.idempotency_key.as_ref() else { return; };
        let Some(fingerprint) = Self::fingerprint(command) else { return; };
        self.replay.lock().unwrap_or_else(|p| p.into_inner()).entry(key.clone()).or_insert_with(|| ReplayRecord {
            fingerprint,
            acknowledgement: acknowledgement.clone(),
        });
    }

    pub fn drain_events(&self, limit: usize, timeout: Duration) -> EventDrain {
        self.events.drain(limit, timeout)
    }

    /// Drain Android platform requests for the JNI adapter.
    pub fn drain_platform_requests(&self, limit: usize, timeout: Duration) -> PlatformRequestBatch {
        self.platform.drain(limit, timeout)
    }

    /// Submit a completed Android platform response.
    pub fn submit_platform_response(&self, response: PlatformResponse) -> Result<(), PlatformPortError> {
        self.platform.submit_response(response)
    }

}
