//! Controller-side Computer Use worker enrollment, separate from Docker
//! execution worker registrations.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerRemoteWorkerEnrollmentV1 {
    pub schema_version: u16,
    pub enabled: bool,
    pub worker_id: String,
    pub machine_id: String,
    /// Operator-provisioned SSH alias and account. These are enrollment data,
    /// never derived from a model request or a Docker worker registration.
    pub ssh_host_alias: String,
    pub ssh_principal: String,
    /// Controller-side OpenSSH trust database and binary are explicit; the
    /// enrollment never falls back to ambient SSH configuration.
    pub known_hosts_path: String,
    pub ssh_binary_path: String,
    pub worker_config_path: String,
    pub ssh_host_key_fingerprint: String,
    pub worker_attestation_key_id: String,
    pub worker_attestation_verifying_key_hex: String,
    pub transport_protocol_version: u16,
    pub guest_protocol_version: u16,
    pub trusted_image_digests: std::collections::BTreeSet<String>,
    pub max_vcpus: u16,
    pub max_memory_bytes: u64,
    pub max_disk_bytes: u64,
    pub max_parallel_sessions: u16,
    pub enrolled_at_unix_ms: u64,
}

impl ComputerRemoteWorkerEnrollmentV1 {
    pub fn validate(&self) -> Result<ed25519_dalek::VerifyingKey, &'static str> {
        if self.schema_version != 1
            || !self.enabled
            || self.enrolled_at_unix_ms == 0
            || !valid_worker_id(&self.worker_id)
            || !canonical_uuid(&self.machine_id)
            || !valid_ssh_alias(&self.ssh_host_alias)
            || !valid_ssh_principal(&self.ssh_principal)
            || !valid_absolute_path(&self.known_hosts_path)
            || !valid_absolute_path(&self.ssh_binary_path)
            || !valid_remote_config_path(&self.worker_config_path)
            || !canonical_uuid(&self.worker_attestation_key_id)
            || !valid_fingerprint(&self.ssh_host_key_fingerprint)
            || self.transport_protocol_version
                != crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1
            || self.guest_protocol_version
                != crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1
            || self.trusted_image_digests.is_empty()
            || self
                .trusted_image_digests
                .iter()
                .any(|value| !valid_digest(value))
            || !(1..=64).contains(&self.max_vcpus)
            || self.max_memory_bytes < 256 * 1024 * 1024
            || self.max_disk_bytes < 512 * 1024 * 1024
            || !(1..=16).contains(&self.max_parallel_sessions)
        {
            return Err("remote Computer Use enrollment policy is invalid");
        }
        let bytes = decode_hex(&self.worker_attestation_verifying_key_hex)
            .ok_or("worker attestation key is malformed")?;
        let raw: [u8; 32] = bytes
            .try_into()
            .map_err(|_| "worker attestation key is malformed")?;
        ed25519_dalek::VerifyingKey::from_bytes(&raw)
            .map_err(|_| "worker attestation key is invalid")
    }

    /// Build the SSH transport settings exclusively from the durable Computer
    /// Use enrollment. The trusted known_hosts path and executable remain
    /// explicit host settings; Docker enrollment is not consulted.
    pub fn ssh_config(
        &self,
    ) -> Result<crate::execution::remote::ssh::SshWorkerConfig, &'static str> {
        self.validate()?;
        Ok(crate::execution::remote::ssh::SshWorkerConfig {
            stable_id: self.worker_id.clone(),
            host_alias: self.ssh_host_alias.clone(),
            ssh_principal: self.ssh_principal.clone(),
            verified_host_key_fingerprint: self.ssh_host_key_fingerprint.clone(),
            known_hosts_path: std::path::PathBuf::from(&self.known_hosts_path),
            ssh_binary: std::path::PathBuf::from(&self.ssh_binary_path),
        })
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
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn valid_ssh_alias(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
fn valid_ssh_principal(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
fn valid_remote_config_path(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 512
        && value.split('/').skip(1).all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        })
}
fn valid_absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 1024
        && value.split('/').skip(1).all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        })
}
fn valid_fingerprint(value: &str) -> bool {
    value.starts_with("SHA256:")
        && value.len() == 50
        && value[7..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
}
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |b: u8| -> Option<u8> {
                Some(match b {
                    b'0'..=b'9' => b - b'0',
                    b'a'..=b'f' => b - b'a' + 10,
                    b'A'..=b'F' => b - b'A' + 10,
                    _ => return None,
                })
            };
            Some(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect()
}
