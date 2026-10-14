use super::*;

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
