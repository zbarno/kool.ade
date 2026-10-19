use super::*;

impl RemoteWorkerClient {
    /// Send one already-authorized secret chunk without creating a durable
    /// request record or attempting reconnect/replay after an ambiguous send.
    pub(crate) fn send_authorized_private_file(
        &mut self,
        execution_id: &str,
        grant: &crate::execution::remote::private_files::RemotePrivateFileGrantV1,
        relative_path: &str,
        source_file: &crate::execution::remote::private_files::AuthorizedPrivateFileBytes,
    ) -> Result<(), ExecutionError> {
        use crate::execution::remote::private_files::{
            MAX_PRIVATE_FILE_CHUNK_BYTES, PrivateFileChunkV1,
        };

        let now = crate::persistence::harness_settings::current_unix_ms();
        grant.validate_at(now).map_err(|_| incompatible())?;
        if grant.worker_id != self.config.stable_id
            || grant.worker_host_key_fingerprint != self.config.verified_host_key_fingerprint
            || grant.controller_id
                != crate::persistence::execution_records::owner_instance_id()
                    .map_err(|_| unavailable())?
        {
            return Err(incompatible());
        }
        crate::persistence::remote_private_file_grants::require_authorized(
            &crate::persistence::state_root(),
            grant,
            now,
        )
        .map_err(|_| incompatible())?;
        grant
            .validate_complete_file(relative_path, source_file.as_slice())
            .map_err(|_| incompatible())?;
        let bytes = source_file.as_slice();
        let mut offset = 0_usize;
        while offset < bytes.len() {
            let end = offset
                .saturating_add(MAX_PRIVATE_FILE_CHUNK_BYTES)
                .min(bytes.len());
            let chunk = PrivateFileChunkV1::new(
                grant.grant_id.clone(),
                relative_path.to_owned(),
                offset as u64,
                bytes[offset..end].to_vec(),
            )
            .map_err(|_| incompatible())?;
            let current_time = crate::persistence::harness_settings::current_unix_ms();
            grant
                .validate_at(current_time)
                .map_err(|_| incompatible())?;
            chunk
                .validate_for_grant(grant, current_time)
                .map_err(|_| incompatible())?;
            self.refresh_health_if_due()?;
            let _grant_use =
                crate::persistence::remote_private_file_grants::lock_authorized_grants(
                    &crate::persistence::state_root(),
                    std::slice::from_ref(grant),
                    current_time,
                )
                .map_err(|_| incompatible())?;
            self.send_private_chunk_frame(execution_id, &chunk)?;
            offset = end;
        }
        Ok(())
    }

    fn send_private_chunk_frame(
        &mut self,
        execution_id: &str,
        chunk: &crate::execution::remote::private_files::PrivateFileChunkV1,
    ) -> Result<(), ExecutionError> {
        use super::super::protocol::EnvelopeKindV1 as Kind;

        let sequence = self
            .sequences
            .get(execution_id)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        let payload = serde_json::to_value(chunk.header().map_err(|_| incompatible())?)
            .map_err(|_| incompatible())?;
        let envelope = EnvelopeV1 {
            version: PROTOCOL_VERSION_V1,
            request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            execution_id: execution_id.to_owned(),
            sequence,
            kind: Kind::PrivateFileChunk,
            payload,
        };
        envelope.validate().map_err(|_| incompatible())?;
        self.sequences.insert(execution_id.to_owned(), sequence);
        if self
            .transport
            .send_private_file_chunk(&envelope, chunk)
            .is_err()
        {
            let _ = self.transport.terminate();
            self.persist_health(
                crate::persistence::remote_workers::RemoteWorkerHealth::Disconnected,
            );
            return Err(self.disconnected_with_diagnostics());
        }
        let response = match self.transport.receive() {
            Ok(Some(response)) => response,
            _ => {
                let _ = self.transport.terminate();
                self.persist_health(
                    crate::persistence::remote_workers::RemoteWorkerHealth::Disconnected,
                );
                return Err(self.disconnected_with_diagnostics());
            }
        };
        if response.request_id != envelope.request_id
            || response.execution_id != envelope.execution_id
            || response.sequence != envelope.sequence
        {
            let _ = self.transport.terminate();
            return Err(incompatible());
        }
        if response.kind == Kind::Error {
            let error: ExecutionError =
                serde_json::from_value(response.payload).map_err(|_| incompatible())?;
            error.validate().map_err(|_| incompatible())?;
            return Err(error);
        }
        if response.kind != Kind::Acknowledge {
            let _ = self.transport.terminate();
            return Err(incompatible());
        }
        Ok(())
    }
}
