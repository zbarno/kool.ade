use serde::{Deserialize, Serialize};

use crate::computer_use::{
    ComputerActionV1, ComputerBackendEventV1, ComputerDisplayFrameV1, ComputerDisplayInputV1,
    ComputerGuestReadinessV1, ComputerMachineHandleV1, ComputerMachineRecordV1, ComputerSessionV1,
    guest_protocol::{GuestActionOutcomeV1, GuestCapabilitySetV1},
};

use crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    content = "body",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ComputerRemoteWorkerRequestBodyV1 {
    Probe,
    Start {
        session: ComputerSessionV1,
    },
    Action {
        action: ComputerActionV1,
    },
    Cancel {
        replacement_session: ComputerSessionV1,
    },
    Recover {
        replacement_session: ComputerSessionV1,
        last_event_sequence: u64,
    },
    Operation {
        operation: ComputerRemoteWorkerOperationV1,
    },
}

/// Typed remote port for the ComputerBackend lifecycle and guest-action path.
/// Image overlays are resolved from the worker's trusted cache, never from a
/// controller-supplied host path.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "operation",
    rename_all = "snake_case",
    content = "arguments",
    deny_unknown_fields
)]
pub enum ComputerRemoteWorkerOperationV1 {
    Boot {
        handle: ComputerMachineHandleV1,
    },
    WaitReady {
        handle: ComputerMachineHandleV1,
        timeout_ms: u32,
    },
    Suspend {
        handle: ComputerMachineHandleV1,
    },
    Resume {
        handle: ComputerMachineHandleV1,
    },
    Stop {
        handle: ComputerMachineHandleV1,
    },
    Collect {
        handle: ComputerMachineHandleV1,
    },
    Snapshot {
        handle: ComputerMachineHandleV1,
    },
    Release {
        handle: ComputerMachineHandleV1,
    },
    CaptureDisplay {
        handle: ComputerMachineHandleV1,
    },
    SendDisplayInput {
        handle: ComputerMachineHandleV1,
        input: ComputerDisplayInputV1,
    },
    AuthenticateGuest {
        handle: ComputerMachineHandleV1,
    },
    MarkGuestReady {
        handle: ComputerMachineHandleV1,
        readiness: ComputerGuestReadinessV1,
    },
    GuestAction {
        handle: ComputerMachineHandleV1,
        action: ComputerActionV1,
        deadline_unix_ms: u64,
    },
    WriteArtifactChunk {
        handle: ComputerMachineHandleV1,
        transfer_id: String,
        file_name: String,
        offset: u64,
        total_bytes: u64,
        sha256: String,
        bytes: Vec<u8>,
    },
    ReadArtifactChunk {
        handle: ComputerMachineHandleV1,
        transfer_id: String,
        file_name: String,
        offset: u64,
    },
    AbortArtifactTransfer {
        handle: ComputerMachineHandleV1,
        transfer_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    content = "value",
    deny_unknown_fields
)]
pub enum ComputerRemoteWorkerResultV1 {
    Advertisement(crate::computer_use::remote_worker::ComputerRemoteWorkerAdvertisementV1),
    Acknowledged,
    Prepared {
        handle: ComputerMachineHandleV1,
        event: ComputerBackendEventV1,
    },
    BackendEvent(ComputerBackendEventV1),
    MachineRecord(ComputerMachineRecordV1),
    DisplayFrame(ComputerDisplayFrameV1),
    GuestAction(GuestActionOutcomeV1),
    GuestAuthenticated {
        readiness: ComputerGuestReadinessV1,
        capabilities: GuestCapabilitySetV1,
        transcript_sha256: String,
    },
    ArtifactWriteAck {
        transfer_id: String,
        next_offset: u64,
        complete: bool,
    },
    ArtifactReadChunk {
        transfer_id: String,
        offset: u64,
        total_bytes: u64,
        sha256: String,
        bytes: Vec<u8>,
        complete: bool,
    },
}

/// Every worker RPC is bound to an enrolled host identity, one Computer Use
/// session, and monotonic controller and lease fences.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerRemoteWorkerRequestV1 {
    pub protocol_version: u16,
    pub worker_id: String,
    pub ssh_host_key_fingerprint: String,
    pub session_id: String,
    pub lease_id: String,
    pub lease_generation: u64,
    pub approval_generation: u64,
    pub cancellation_generation: u64,
    pub transport_sequence: u64,
    pub request_id: String,
    pub body: ComputerRemoteWorkerRequestBodyV1,
}

impl ComputerRemoteWorkerRequestV1 {
    pub fn validate_shape(&self) -> Result<(), &'static str> {
        if self.protocol_version != COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1
            || !valid_worker_id(&self.worker_id)
            || !valid_fingerprint(&self.ssh_host_key_fingerprint)
            || !canonical_uuid(&self.session_id)
            || !canonical_uuid(&self.lease_id)
            || !canonical_uuid(&self.request_id)
            || self.lease_generation == 0
            || self.approval_generation == 0
            || self.cancellation_generation == 0
            || (self.transport_sequence == 0
                && !matches!(
                    &self.body,
                    ComputerRemoteWorkerRequestBodyV1::Recover { .. }
                ))
        {
            return Err("remote Computer Use request identity is invalid");
        }
        if let ComputerRemoteWorkerRequestBodyV1::Operation { operation } = &self.body {
            match operation {
                ComputerRemoteWorkerOperationV1::WriteArtifactChunk {
                    transfer_id,
                    file_name,
                    offset,
                    total_bytes,
                    sha256,
                    bytes,
                    ..
                } => {
                    if !canonical_uuid(transfer_id)
                        || !valid_artifact_name(file_name)
                        || *total_bytes == 0
                        || *total_bytes > 64 * 1024 * 1024
                        || bytes.is_empty()
                        || bytes.len() > 256 * 1024
                        || offset
                            .checked_add(bytes.len() as u64)
                            .is_none_or(|end| end > *total_bytes)
                        || !valid_artifact_digest(sha256)
                    {
                        return Err("remote artifact write chunk is invalid or exceeds bounds");
                    }
                }
                ComputerRemoteWorkerOperationV1::ReadArtifactChunk {
                    transfer_id,
                    file_name,
                    offset,
                    ..
                } => {
                    if !canonical_uuid(transfer_id)
                        || !valid_artifact_name(file_name)
                        || *offset > 64 * 1024 * 1024
                    {
                        return Err("remote artifact read request is invalid or exceeds bounds");
                    }
                }
                ComputerRemoteWorkerOperationV1::AbortArtifactTransfer { transfer_id, .. }
                    if !canonical_uuid(transfer_id) =>
                {
                    return Err("remote artifact abort request is invalid");
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerRemoteWorkerResponseV1 {
    pub protocol_version: u16,
    pub worker_id: String,
    pub session_id: String,
    pub lease_id: String,
    pub lease_generation: u64,
    pub transport_sequence: u64,
    pub result: ComputerRemoteWorkerResultV1,
}

impl ComputerRemoteWorkerResponseV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.protocol_version != COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1
            || !valid_worker_id(&self.worker_id)
            || !canonical_uuid(&self.session_id)
            || !canonical_uuid(&self.lease_id)
            || self.lease_generation == 0
        {
            return Err("remote Computer Use response identity is invalid");
        }
        match &self.result {
            ComputerRemoteWorkerResultV1::Advertisement(advertisement) => {
                if advertisement.worker_id != self.worker_id
                    || advertisement.ssh_host_key_fingerprint.is_empty()
                {
                    return Err("remote worker advertisement identity is invalid");
                }
                advertisement
                    .validate()
                    .map_err(|_| "remote worker advertisement is invalid")
            }
            ComputerRemoteWorkerResultV1::Acknowledged => Ok(()),
            ComputerRemoteWorkerResultV1::Prepared { handle, event } => {
                if handle.session_id() != self.session_id
                    || handle.lease_id() != self.lease_id
                    || handle.lease_generation() != self.lease_generation
                    || event.session_id != self.session_id
                    || event.lease_generation != self.lease_generation
                {
                    return Err("remote worker prepared result identity is invalid");
                }
                Ok(())
            }
            ComputerRemoteWorkerResultV1::BackendEvent(event) => {
                if event.session_id != self.session_id
                    || event.lease_generation != self.lease_generation
                {
                    return Err("remote worker event identity is invalid");
                }
                Ok(())
            }
            ComputerRemoteWorkerResultV1::MachineRecord(record) => {
                if record.session_id != self.session_id
                    || record.lease_id != self.lease_id
                    || record.lease_generation != self.lease_generation
                {
                    return Err("remote worker machine record identity is invalid");
                }
                Ok(())
            }
            ComputerRemoteWorkerResultV1::DisplayFrame(frame) => {
                if !(1..=16_384).contains(&frame.width)
                    || !(1..=16_384).contains(&frame.height)
                    || frame.png.is_empty()
                    || frame.png.len() > 768 * 1024
                {
                    return Err("remote worker display frame exceeds bounds");
                }
                Ok(())
            }
            ComputerRemoteWorkerResultV1::GuestAction(outcome) => {
                if outcome.observation.as_ref().is_some_and(|observation| {
                    observation.session_id != self.session_id
                        || observation.validate_metadata().is_err()
                }) {
                    return Err("remote worker action observation is invalid");
                }
                Ok(())
            }
            ComputerRemoteWorkerResultV1::GuestAuthenticated {
                readiness,
                capabilities,
                transcript_sha256,
            } => {
                if readiness.session_id != self.session_id
                    || readiness.lease_generation != self.lease_generation
                    || readiness.guest_boot_id.is_empty()
                    || !canonical_uuid(&readiness.guest_boot_id)
                    || !valid_sha256_hex(transcript_sha256)
                    || capabilities.validate().is_err()
                {
                    return Err("remote worker guest authentication result is invalid");
                }
                Ok(())
            }
            ComputerRemoteWorkerResultV1::ArtifactWriteAck {
                transfer_id,
                next_offset,
                ..
            } => {
                if !canonical_uuid(transfer_id) || *next_offset > 64 * 1024 * 1024 {
                    return Err("remote artifact write acknowledgement is invalid");
                }
                Ok(())
            }
            ComputerRemoteWorkerResultV1::ArtifactReadChunk {
                transfer_id,
                offset,
                total_bytes,
                sha256,
                bytes,
                complete,
            } => {
                if !canonical_uuid(transfer_id)
                    || *total_bytes == 0
                    || *total_bytes > 64 * 1024 * 1024
                    || bytes.len() > 256 * 1024
                    || *offset > *total_bytes
                    || offset.saturating_add(bytes.len() as u64) > *total_bytes
                    || *complete != (offset.saturating_add(bytes.len() as u64) == *total_bytes)
                    || !valid_artifact_digest(sha256)
                {
                    return Err("remote artifact read chunk exceeds its bound");
                }
                Ok(())
            }
        }
    }
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn valid_worker_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_fingerprint(value: &str) -> bool {
    value.starts_with("SHA256:")
        && value.len() == 50
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
}

fn valid_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_artifact_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('.')
        && !value.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        && !value.chars().any(char::is_control)
        && !matches!(value, "." | "..")
}

fn valid_artifact_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(file_name: &str, bytes: Vec<u8>) -> ComputerRemoteWorkerRequestV1 {
        let session_id = uuid::Uuid::new_v4().to_string();
        let transfer_id = uuid::Uuid::new_v4().to_string();
        ComputerRemoteWorkerRequestV1 {
            protocol_version: COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
            worker_id: "worker-one".into(),
            ssh_host_key_fingerprint: format!("SHA256:{}", "A".repeat(43)),
            session_id: session_id.clone(),
            lease_id: uuid::Uuid::new_v4().to_string(),
            lease_generation: 1,
            approval_generation: 1,
            cancellation_generation: 1,
            transport_sequence: 1,
            request_id: uuid::Uuid::new_v4().to_string(),
            body: ComputerRemoteWorkerRequestBodyV1::Operation {
                operation: ComputerRemoteWorkerOperationV1::WriteArtifactChunk {
                    handle: ComputerMachineHandleV1::from_remote(
                        session_id,
                        uuid::Uuid::new_v4().to_string(),
                        1,
                        uuid::Uuid::new_v4().to_string(),
                    ),
                    transfer_id,
                    file_name: file_name.into(),
                    offset: 0,
                    total_bytes: bytes.len() as u64,
                    sha256: format!("sha256:{}", "a".repeat(64)),
                    bytes,
                },
            },
        }
    }

    #[test]
    fn artifact_rpc_chunks_are_pathless_and_bounded() {
        assert!(
            request("report.txt", vec![b'x'; 256 * 1024])
                .validate_shape()
                .is_ok()
        );
        assert!(
            request("../secret.txt", vec![b'x'; 3])
                .validate_shape()
                .is_err()
        );
        assert!(
            request("report.txt", vec![b'x'; 256 * 1024 + 1])
                .validate_shape()
                .is_err()
        );
    }
}
