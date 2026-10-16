use super::*;

impl RemoteWorkerClient {
    pub(super) fn health_error_with_diagnostics(&self, error: ExecutionError) -> ExecutionError {
        let mut diagnostic = self
            .transport
            .diagnostic_stderr()
            .chars()
            .filter(|character| !character.is_control())
            .take(512)
            .collect::<String>();
        while diagnostic.len() > 512 {
            diagnostic.pop();
        }
        if diagnostic.is_empty() {
            return error;
        }
        let mut message = error.message;
        while message.len() > 3_500 {
            message.pop();
        }
        ExecutionError::new(
            error.kind,
            format!("{message}; SSH worker diagnostic: {diagnostic}"),
            error.retryable,
        )
    }

    pub(super) fn reconnect_and_negotiate(&mut self) -> Result<(), ExecutionError> {
        let config = self.config.clone();
        self.reconnect_and_negotiate_with(
            || {
                let worker = verified(&config)?;
                SshTransport::connect(&worker)
                    .map(|transport| Box::new(transport) as Box<dyn RemoteRpcTransport>)
                    .map_err(|_| disconnected())
            },
            true,
        )
    }

    pub(super) fn reconnect_and_negotiate_with(
        &mut self,
        connect: impl FnOnce() -> Result<Box<dyn RemoteRpcTransport>, ExecutionError>,
        record_health: bool,
    ) -> Result<(), ExecutionError> {
        self.transport = connect()?;
        self.system_image_capability_key = new_capability_key();
        let request = EnvelopeV1 {
            version: PROTOCOL_VERSION_V1,
            request_id: uuid::Uuid::new_v4().to_string(),
            execution_id: SYSTEM_IMAGE_SESSION_ID_V1.to_owned(),
            sequence: 1,
            kind: Kind::Health,
            payload: serde_json::json!({
                "wire_version": PROTOCOL_VERSION_V1,
                "system_image_key": self.system_image_key_hex(),
            }),
        };
        self.transport.send(&request).map_err(|_| disconnected())?;
        let response = self
            .transport
            .receive()
            .map_err(|_| disconnected())?
            .ok_or_else(disconnected)?;
        if response.request_id != request.request_id
            || response.execution_id != request.execution_id
            || response.sequence != request.sequence
        {
            return Err(incompatible());
        }
        if response.kind == Kind::Error {
            let error: ExecutionError =
                serde_json::from_value(response.payload.clone()).map_err(|_| incompatible())?;
            error.validate().map_err(|_| incompatible())?;
            return Err(error);
        }
        if response.kind != Kind::Acknowledge {
            return Err(incompatible());
        }
        self.probe = match negotiated_probe(response.payload) {
            Ok(probe) => probe,
            Err(error) => {
                self.persist_health(
                    crate::persistence::remote_workers::RemoteWorkerHealth::Incompatible,
                );
                return Err(error);
            }
        };
        if record_health {
            self.record_current_health();
        }
        self.last_health_check = Instant::now();
        // Keep this exact authenticated SSH stream: it is the worker session
        // that now holds the freshly rotated capability key. The reserved
        // handshake execution ID avoids colliding with replayed control calls.
        Ok(())
    }
}

impl RemoteWorkerClient {
    pub(super) fn disconnected_with_diagnostics(&self) -> ExecutionError {
        let diagnostic = self
            .transport
            .diagnostic_stderr()
            .chars()
            .filter(|character| !character.is_control())
            .take(512)
            .collect::<String>();
        if diagnostic.is_empty() {
            disconnected()
        } else {
            ExecutionError::new(
                ExecutionErrorKind::WorkerDisconnected,
                format!("Remote worker connection failed: {diagnostic}"),
                true,
            )
        }
    }

    pub(super) fn replay_pending_requests(&mut self) -> Result<(), ExecutionError> {
        for pending in remote_requests::pending(&self.worker_id).map_err(|_| unavailable())? {
            self.sequences
                .entry(pending.envelope.execution_id.clone())
                .and_modify(|value| *value = (*value).max(pending.envelope.sequence))
                .or_insert(pending.envelope.sequence);
            // A fresh transport permits the durable request's original sequence to be
            // replayed exactly even when the previous session died mid-response.
            self.reconnect_and_negotiate()?;
            let response = self
                .send_envelope(&pending.envelope)
                .map_err(|_| disconnected())?;
            if response.kind == Kind::Error {
                let error: ExecutionError =
                    serde_json::from_value(response.payload.clone()).map_err(|_| incompatible())?;
                if error.kind == ExecutionErrorKind::WorkerDisconnected {
                    continue;
                }
            }
            remote_requests::record_completion(
                &self.worker_id,
                &pending.envelope.request_id,
                &response,
            )
            .map_err(|_| unavailable())?;
        }
        Ok(())
    }
}
