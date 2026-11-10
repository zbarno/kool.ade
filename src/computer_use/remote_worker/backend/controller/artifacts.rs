use super::*;

impl RemoteComputerBackendV1 {
    pub(crate) fn write_artifact_to_guest(
        &self,
        session: &ComputerSessionV1,
        file_name: &str,
        bytes: &[u8],
        now_unix_ms: u64,
        mut authorize_chunk: impl FnMut() -> bool,
    ) -> Result<String, Error> {
        self.validate_session(session, now_unix_ms)?;
        if !valid_artifact_name(file_name)
            || bytes.is_empty()
            || bytes.len() as u64 > crate::computer_use::artifacts::COMPUTER_ARTIFACT_MAX_BYTES_V1
        {
            return Err(Error::UnsupportedPolicy);
        }
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let digest = format!("sha256:{:x}", sha2::Sha256::digest(bytes));
        let mut offset = 0usize;
        while offset < bytes.len() {
            if !authorize_chunk() {
                self.abort_artifact_transfer(session, &transfer_id);
                return Err(Error::UnsupportedPolicy);
            }
            let end = (offset + 256 * 1024).min(bytes.len());
            let result = match self.send_operation(
                session,
                Operation::WriteArtifactChunk {
                    handle: self.remote_handle(session)?,
                    transfer_id: transfer_id.clone(),
                    file_name: file_name.to_owned(),
                    offset: offset as u64,
                    total_bytes: bytes.len() as u64,
                    sha256: digest.clone(),
                    bytes: bytes[offset..end].to_vec(),
                },
            ) {
                Ok(result) => result,
                Err(error) => {
                    self.abort_artifact_transfer(session, &transfer_id);
                    return Err(error);
                }
            };
            let ResultValue::ArtifactWriteAck {
                transfer_id: ack_id,
                next_offset,
                complete,
            } = result
            else {
                return Err(Error::Io);
            };
            if ack_id != transfer_id
                || next_offset != end as u64
                || complete != (end == bytes.len())
            {
                self.abort_artifact_transfer(session, &transfer_id);
                return Err(Error::StaleLease);
            }
            offset = end;
        }
        Ok(transfer_id)
    }

    pub(crate) fn read_artifact_from_guest(
        &self,
        session: &ComputerSessionV1,
        file_name: &str,
        max_bytes: u64,
        now_unix_ms: u64,
        mut authorize_chunk: impl FnMut() -> bool,
    ) -> Result<(Vec<u8>, String), Error> {
        self.validate_session(session, now_unix_ms)?;
        if !valid_artifact_name(file_name) {
            return Err(Error::UnsupportedPolicy);
        }
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let mut offset = 0u64;
        let mut expected_total = None;
        let mut expected_digest = None;
        let mut bytes = Vec::new();
        loop {
            if !authorize_chunk() {
                return Err(Error::UnsupportedPolicy);
            }
            let result = self.send_operation(
                session,
                Operation::ReadArtifactChunk {
                    handle: self.remote_handle(session)?,
                    transfer_id: transfer_id.clone(),
                    file_name: file_name.to_owned(),
                    offset,
                },
            )?;
            let ResultValue::ArtifactReadChunk {
                transfer_id: response_id,
                offset: response_offset,
                total_bytes,
                sha256,
                bytes: chunk,
                complete,
            } = result
            else {
                return Err(Error::Io);
            };
            if response_id != transfer_id
                || response_offset != offset
                || total_bytes == 0
                || total_bytes > max_bytes
                || total_bytes > crate::computer_use::artifacts::COMPUTER_ARTIFACT_MAX_BYTES_V1
                || chunk.is_empty()
                || chunk.len() > 256 * 1024
                || offset.saturating_add(chunk.len() as u64) > total_bytes
                || complete != (offset + chunk.len() as u64 == total_bytes)
                || expected_total.is_some_and(|expected| expected != total_bytes)
                || expected_digest
                    .as_ref()
                    .is_some_and(|expected| expected != &sha256)
            {
                return Err(Error::StaleLease);
            }
            expected_total = Some(total_bytes);
            expected_digest = Some(sha256);
            offset += chunk.len() as u64;
            bytes.extend_from_slice(&chunk);
            if complete {
                break;
            }
        }
        let digest = format!("sha256:{:x}", sha2::Sha256::digest(&bytes));
        let expected = expected_digest.ok_or(Error::Io)?;
        if expected != digest || expected_total != Some(bytes.len() as u64) {
            return Err(Error::Io);
        }
        Ok((bytes, digest))
    }

    pub(super) fn abort_artifact_transfer(&self, session: &ComputerSessionV1, transfer_id: &str) {
        if let Ok(handle) = self.remote_handle(session) {
            let _ = self.send_operation(
                session,
                Operation::AbortArtifactTransfer {
                    handle,
                    transfer_id: transfer_id.to_owned(),
                },
            );
        }
    }
}
