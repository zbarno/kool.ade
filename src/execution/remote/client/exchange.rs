use super::*;

impl RemoteWorkerClient {
    pub(super) fn exchange(
        &mut self,
        kind: Kind,
        execution_id: &str,
        payload: serde_json::Value,
    ) -> Result<EnvelopeV1, ExecutionError> {
        let is_health = kind == Kind::Health;
        let sequence = self
            .sequences
            .get(execution_id)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        let envelope = EnvelopeV1 {
            version: PROTOCOL_VERSION_V1,
            request_id: uuid::Uuid::new_v4().to_string(),
            execution_id: execution_id.to_owned(),
            sequence,
            kind,
            payload,
        };
        // Health carries the session-only capability key. Keep that secret out
        // of the durable retry journal; health negotiation is safely retried
        // over the current verified SSH connection instead.
        if !is_health {
            remote_requests::save_pending(&self.worker_id, &envelope).map_err(|_| unavailable())?;
        }
        self.sequences.insert(execution_id.to_owned(), sequence);
        let response = match self.send_envelope(&envelope) {
            Ok(response) => response,
            Err(_) => {
                if self.reconnect_and_negotiate().is_err() {
                    self.persist_health(
                        crate::persistence::remote_workers::RemoteWorkerHealth::Disconnected,
                    );
                    return Err(self.disconnected_with_diagnostics());
                }
                if is_health {
                    // Reconnect negotiation already completed Health with the
                    // freshly rotated key on this stream. Return that probe
                    // instead of replaying an old bootstrap sequence/key.
                    return Ok(EnvelopeV1 {
                        version: PROTOCOL_VERSION_V1,
                        request_id: envelope.request_id,
                        execution_id: envelope.execution_id,
                        sequence: envelope.sequence,
                        kind: Kind::Acknowledge,
                        payload: serde_json::json!({
                            "wire_version": PROTOCOL_VERSION_V1,
                            "probe": self.probe,
                        }),
                    });
                }
                match self.send_envelope(&envelope) {
                    Ok(response) => response,
                    Err(_) => {
                        self.persist_health(
                            crate::persistence::remote_workers::RemoteWorkerHealth::Disconnected,
                        );
                        return Err(self.disconnected_with_diagnostics());
                    }
                }
            }
        };
        self.finish_response(&envelope, response)
    }

    pub(super) fn send_envelope(
        &mut self,
        envelope: &EnvelopeV1,
    ) -> Result<EnvelopeV1, std::io::Error> {
        self.transport.send(envelope)?;
        let response = self.transport.receive()?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "worker closed the protocol stream",
            )
        })?;
        if response.request_id != envelope.request_id
            || response.execution_id != envelope.execution_id
            || response.sequence != envelope.sequence
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "worker response identity does not match request",
            ));
        }
        if response.kind != Kind::Error && !matches_response(&envelope.kind, &response.kind) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "worker response kind does not match request",
            ));
        }
        Ok(response)
    }

    fn finish_response(
        &self,
        request: &EnvelopeV1,
        response: EnvelopeV1,
    ) -> Result<EnvelopeV1, ExecutionError> {
        if response.kind == Kind::Error {
            let error: ExecutionError =
                serde_json::from_value(response.payload.clone()).map_err(|parse_error| {
                    ExecutionError::new(
                        ExecutionErrorKind::WorkerIncompatible,
                        format!("Remote worker returned an invalid error response: {parse_error}"),
                        false,
                    )
                })?;
            error.validate().map_err(|validation_error| {
                ExecutionError::new(
                    ExecutionErrorKind::WorkerIncompatible,
                    format!("Remote worker returned an invalid error response: {validation_error}"),
                    false,
                )
            })?;
            if request.kind != Kind::Health && error.kind != ExecutionErrorKind::WorkerDisconnected
            {
                remote_requests::record_completion(&self.worker_id, &request.request_id, &response)
                    .map_err(|_| unavailable())?;
            }
            return Err(error);
        }
        if request.kind != Kind::Health {
            remote_requests::record_completion(&self.worker_id, &request.request_id, &response)
                .map_err(|_| unavailable())?;
        }
        Ok(response)
    }
}
