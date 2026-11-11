//! Explicit, operator-provisioned trust roots for the remote Computer Use VM
//! worker. Its private key is accepted only from the worker's owner-private file.

use std::{collections::BTreeSet, fs, io::Read, path::Path};

use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::computer_use::{ComputerRemoteWorkerTrustV1, ComputerTargetV1};

const MAX_CONFIG_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerRemoteWorkerHostConfigV1 {
    pub schema_version: u16,
    pub enabled: bool,
    pub worker_id: String,
    pub machine_id: String,
    pub ssh_host_key_fingerprint: String,
    pub worker_attestation_key_id: String,
    /// Private worker attestation seed; this file is stored only on worker.
    pub worker_attestation_private_key_hex: String,
    pub controller_key_id: String,
    /// Ed25519 controller verifying key in lower-case hex. Never a signing key.
    pub controller_verifying_key_hex: String,
    /// Explicit worker-local paths for the signed catalog, durable cache/floor,
    /// runtime roots and fence checkpoint. None are inferred by the service.
    pub catalog_path: std::path::PathBuf,
    pub image_cache_root: std::path::PathBuf,
    pub generation_floor_root: std::path::PathBuf,
    pub overlay_root: std::path::PathBuf,
    pub runtime_root: std::path::PathBuf,
    pub cgroup_root: std::path::PathBuf,
    pub checkpoint_path: std::path::PathBuf,
    pub trusted_image_digests: BTreeSet<String>,
    pub max_vcpus: u16,
    pub max_memory_bytes: u64,
    pub max_disk_bytes: u64,
    pub max_parallel_sessions: u16,
}

impl ComputerRemoteWorkerHostConfigV1 {
    pub fn validate(&self) -> Result<VerifyingKey, &'static str> {
        if self.schema_version != 1 || !self.enabled {
            return Err("remote Computer Use worker is disabled or has an unsupported schema");
        }
        if !valid_worker_id(&self.worker_id)
            || !canonical_uuid(&self.machine_id)
            || !canonical_uuid(&self.controller_key_id)
            || !canonical_uuid(&self.worker_attestation_key_id)
        {
            return Err("remote Computer Use worker identity is invalid");
        }
        if [
            &self.catalog_path,
            &self.image_cache_root,
            &self.generation_floor_root,
            &self.overlay_root,
            &self.runtime_root,
            &self.cgroup_root,
            &self.checkpoint_path,
        ]
        .iter()
        .any(|path| !path.is_absolute() || path.as_os_str().is_empty())
        {
            return Err("remote worker state paths must be explicitly absolute");
        }
        let worker_seed = decode_hex(&self.worker_attestation_private_key_hex)
            .ok_or("worker attestation seed is malformed")?;
        let mut worker_seed: [u8; 32] = worker_seed
            .try_into()
            .map_err(|_| "worker attestation seed is malformed")?;
        let worker_key = ed25519_dalek::SigningKey::from_bytes(&worker_seed);
        worker_seed.zeroize();
        let trust = ComputerRemoteWorkerTrustV1 {
            ssh_host_key_fingerprint: self.ssh_host_key_fingerprint.clone(),
            worker_attestation_key_id: self.worker_attestation_key_id.clone(),
            worker_attestation_verifying_key_hex: hex_encode(
                &worker_key.verifying_key().to_bytes(),
            ),
            transport_protocol_version:
                crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
            guest_protocol_version: crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
        };
        trust.validate()?;
        if !(1..=64).contains(&self.max_vcpus)
            || !(256 * 1024 * 1024..=1_099_511_627_776).contains(&self.max_memory_bytes)
            || !(512 * 1024 * 1024..=4_398_046_511_104).contains(&self.max_disk_bytes)
            || !(1..=16).contains(&self.max_parallel_sessions)
            || self.trusted_image_digests.is_empty()
            || self.trusted_image_digests.len() > 128
            || self
                .trusted_image_digests
                .iter()
                .any(|digest| !valid_digest(digest))
        {
            return Err("remote Computer Use worker policy bounds are invalid");
        }
        let bytes = decode_hex(&self.controller_verifying_key_hex)
            .ok_or("controller verifying key is malformed")?;
        let raw: [u8; 32] = bytes
            .try_into()
            .map_err(|_| "controller verifying key is malformed")?;
        let controller_key =
            VerifyingKey::from_bytes(&raw).map_err(|_| "controller verifying key is invalid")?;
        Ok(controller_key)
    }

    pub fn target(&self) -> Result<ComputerTargetV1, &'static str> {
        self.validate()?;
        let worker_seed = decode_hex(&self.worker_attestation_private_key_hex)
            .ok_or("worker attestation seed is malformed")?;
        let mut worker_seed: [u8; 32] = worker_seed
            .try_into()
            .map_err(|_| "worker attestation seed is malformed")?;
        let worker_key = ed25519_dalek::SigningKey::from_bytes(&worker_seed);
        worker_seed.zeroize();
        Ok(ComputerTargetV1::RemoteVmWorker {
            worker_id: self.worker_id.clone(),
            machine_id: self.machine_id.clone(),
            trust: ComputerRemoteWorkerTrustV1 {
                ssh_host_key_fingerprint: self.ssh_host_key_fingerprint.clone(),
                worker_attestation_key_id: self.worker_attestation_key_id.clone(),
                worker_attestation_verifying_key_hex: hex_encode(
                    &worker_key.verifying_key().to_bytes(),
                ),
                transport_protocol_version:
                    crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
                guest_protocol_version:
                    crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
            },
        })
    }

    pub fn sign_advertisement(
        &self,
        advertisement: &mut super::ComputerRemoteWorkerAdvertisementV1,
    ) -> Result<(), &'static str> {
        self.validate()?;
        let seed = decode_hex(&self.worker_attestation_private_key_hex)
            .ok_or("worker attestation seed is malformed")?;
        let mut seed: [u8; 32] = seed
            .try_into()
            .map_err(|_| "worker attestation seed is malformed")?;
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
        seed.zeroize();
        advertisement.attest(self.worker_attestation_key_id.clone(), &signing_key)
    }
}

/// Load only an explicitly named, owner-private regular file. The worker
/// never invents a trust root or silently falls back to a default key.
pub fn load_computer_remote_worker_host_config_v1(
    path: &Path,
) -> anyhow::Result<ComputerRemoteWorkerHostConfigV1> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "worker config must be a regular file"
    );
    anyhow::ensure!(
        metadata.len() <= MAX_CONFIG_BYTES,
        "worker config exceeds size limit"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() },
            "worker config must be owned by this user"
        );
        anyhow::ensure!(
            metadata.mode() & 0o077 == 0,
            "worker config permissions must be 0600 or stricter"
        );
    }
    let file = fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_CONFIG_BYTES,
        "worker config exceeds size limit"
    );
    let config: ComputerRemoteWorkerHostConfigV1 = serde_json::from_slice(&bytes)?;
    config.validate().map_err(anyhow::Error::msg)?;
    Ok(config)
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

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
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

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 15) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ComputerRemoteWorkerHostConfigV1 {
        let controller = ed25519_dalek::SigningKey::from_bytes(&[3; 32]);
        ComputerRemoteWorkerHostConfigV1 {
            schema_version: 1,
            enabled: true,
            worker_id: "worker-east".into(),
            machine_id: "00000000-0000-4000-8000-000000000098".into(),
            ssh_host_key_fingerprint: format!("SHA256:{}", "A".repeat(43)),
            worker_attestation_key_id: "00000000-0000-4000-8000-000000000009".into(),
            worker_attestation_private_key_hex: "09".repeat(32),
            controller_key_id: "00000000-0000-4000-8000-000000000008".into(),
            controller_verifying_key_hex: hex_encode(controller.verifying_key().as_bytes()),
            catalog_path: "/etc/koolade/catalog.json".into(),
            image_cache_root: "/var/lib/koolade/images".into(),
            generation_floor_root: "/var/lib/koolade/floor".into(),
            overlay_root: "/var/lib/koolade/overlays".into(),
            runtime_root: "/var/lib/koolade/runtime".into(),
            cgroup_root: "/sys/fs/cgroup/koolade".into(),
            checkpoint_path: "/var/lib/koolade/checkpoint.json".into(),
            trusted_image_digests: [format!("sha256:{}", "a".repeat(64))].into_iter().collect(),
            max_vcpus: 4,
            max_memory_bytes: 1024 * 1024 * 1024,
            max_disk_bytes: 4 * 1024 * 1024 * 1024,
            max_parallel_sessions: 1,
        }
    }

    #[test]
    fn host_config_requires_explicit_absolute_worker_paths() {
        assert!(config().validate().is_ok());
        let mut invalid = config();
        invalid.checkpoint_path = "checkpoint.json".into();
        assert!(invalid.validate().is_err());
    }
}
