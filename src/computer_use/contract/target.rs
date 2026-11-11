use serde::{Deserialize, Serialize};

/// Protocol implemented by the distinct remote Computer Use worker transport.
/// This is not the Docker execution worker protocol.
pub const COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1: u16 = 1;

/// Local hypervisors are explicit so target identity does not imply that all
/// local VM implementations share the KVM security and readiness contract.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerHypervisorV1 {
    QemuKvm,
}

/// Device and wire identity pinned by the controller for one enrolled remote
/// VM worker. Capability reports remain observations and are validated
/// against controller policy before scheduling.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerRemoteWorkerTrustV1 {
    pub ssh_host_key_fingerprint: String,
    pub worker_attestation_key_id: String,
    pub worker_attestation_verifying_key_hex: String,
    pub transport_protocol_version: u16,
    pub guest_protocol_version: u16,
}

impl ComputerRemoteWorkerTrustV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_ssh_fingerprint(&self.ssh_host_key_fingerprint) {
            return Err("remote worker host-key fingerprint is invalid");
        }
        if !canonical_uuid(&self.worker_attestation_key_id)
            || self.worker_attestation_verifying_key_hex.len() != 64
            || !self
                .worker_attestation_verifying_key_hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err("remote worker attestation identity is invalid");
        }
        let key_bytes = decode_hex(&self.worker_attestation_verifying_key_hex)
            .ok_or("remote worker attestation key is malformed")?;
        let key_bytes: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| "remote worker attestation key is malformed")?;
        ed25519_dalek::VerifyingKey::from_bytes(&key_bytes)
            .map_err(|_| "remote worker attestation key is invalid")?;
        if self.transport_protocol_version != COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1 {
            return Err("remote Computer Use worker protocol is unsupported");
        }
        if self.guest_protocol_version != super::super::guest_protocol::GUEST_PROTOCOL_VERSION_V1 {
            return Err("remote guest companion protocol is unsupported");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComputerTargetV1 {
    /// Legacy v1 wire spelling retained for persisted local KVM sessions.
    LocalKvm { machine_id: String },
    LocalVm {
        machine_id: String,
        hypervisor: ComputerHypervisorV1,
    },
    RemoteVmWorker {
        worker_id: String,
        machine_id: String,
        trust: ComputerRemoteWorkerTrustV1,
    },
}

impl ComputerTargetV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::LocalKvm { machine_id } => {
                if !canonical_uuid(machine_id) {
                    return Err("local machine ID is invalid");
                }
            }
            Self::LocalVm {
                machine_id,
                hypervisor: ComputerHypervisorV1::QemuKvm,
            } => {
                if !canonical_uuid(machine_id) {
                    return Err("local machine ID is invalid");
                }
            }
            Self::RemoteVmWorker {
                worker_id,
                machine_id,
                trust,
            } => {
                if !valid_worker_id(worker_id) || !canonical_uuid(machine_id) {
                    return Err("remote worker ID is invalid");
                }
                trust.validate()?;
            }
        }
        Ok(())
    }

    /// Stable worker identity bound into guest protocol messages.
    pub fn worker_id(&self) -> &str {
        match self {
            Self::LocalKvm { machine_id } | Self::LocalVm { machine_id, .. } => machine_id,
            Self::RemoteVmWorker { worker_id, .. } => worker_id,
        }
    }

    /// Identity of the VM host expected by the authenticated guest companion.
    pub fn guest_worker_id(&self) -> &str {
        self.target_machine_id()
    }

    /// Local backends must use this accessor and reject `None`; a remote
    /// worker identity is never a local QEMU machine ID.
    pub fn local_machine_id(&self) -> Option<&str> {
        match self {
            Self::LocalKvm { machine_id } | Self::LocalVm { machine_id, .. } => Some(machine_id),
            Self::RemoteVmWorker { .. } => None,
        }
    }

    pub fn target_machine_id(&self) -> &str {
        match self {
            Self::LocalKvm { machine_id }
            | Self::LocalVm { machine_id, .. }
            | Self::RemoteVmWorker { machine_id, .. } => machine_id,
        }
    }

    pub fn is_remote_vm_worker(&self) -> bool {
        matches!(self, Self::RemoteVmWorker { .. })
    }
}

fn valid_ssh_fingerprint(value: &str) -> bool {
    value.starts_with("SHA256:")
        && value.len() == 50
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
}

fn valid_worker_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.to_string() == value)
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| -> Option<u8> {
                Some(match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    b'A'..=b'F' => byte - b'A' + 10,
                    _ => return None,
                })
            };
            Some(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect()
}

#[cfg(test)]
mod tests;
