use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::app::{
    bootstrap_remediation::HostIdentity,
    bootstrap_repair_policy::{Host, Proposal},
};
use crate::execution::docker::images::{
    DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES, DESKTOP_CAPACITY_PROBE_MAX_STORAGE_BYTES,
};

const SCAN_TTL_MS: u64 = 5 * 60 * 1000;

mod engine;
pub(super) use engine::VerifiedEngine;

#[derive(Clone, Debug, Serialize)]
pub(super) struct MachineBinding {
    pub(super) host: Host,
    pub(super) machine_hash: String,
    pub(super) principal_hash: String,
    pub(super) probe_hash: String,
    pub(super) engine_identity_fingerprint: String,
    pub(super) probed_at_unix_ms: u64,
    pub(super) daemon_id: String,
    pub(super) daemon_version: String,
    pub(super) image_profile: String,
    pub(super) image_reference: String,
    pub(super) image_digest: String,
    pub(super) image_platform: String,
    pub(super) estimated_pull_bytes: u64,
    pub(super) estimated_storage_bytes: u64,
    pub(super) disk_available_bytes: Option<u64>,
    pub(super) minimum_disk_bytes: u64,
    pub(super) capacity_probe_required: bool,
}

#[allow(dead_code)] // Current host evidence is first consumed by the #345 UI.
impl MachineBinding {
    pub(super) async fn current(proposal: &Proposal) -> anyhow::Result<Self> {
        Ok(VerifiedEngine::current(proposal).await?.into_binding())
    }

    pub(super) fn from_facts(
        host: Host,
        machine_id: &str,
        principal: &str,
        probe_hash: &str,
        probed_at_unix_ms: u64,
    ) -> anyhow::Result<Self> {
        let machine_hash = fingerprint(&("koolade-machine-v1", host, machine_id))?;
        let principal_hash = fingerprint(&("koolade-principal-v1", &machine_hash, principal))?;
        Ok(Self::from_hashes(
            host,
            &machine_hash,
            &principal_hash,
            probe_hash,
            probed_at_unix_ms,
        ))
    }

    pub(super) fn from_host_identity(
        identity: &HostIdentity,
        probe_hash: &str,
        probed_at_unix_ms: u64,
    ) -> anyhow::Result<Self> {
        identity.validate()?;
        Ok(Self::from_hashes(
            identity.host(),
            identity.machine_fingerprint(),
            identity.principal_fingerprint(),
            probe_hash,
            probed_at_unix_ms,
        ))
    }

    fn from_hashes(
        host: Host,
        machine_hash: &str,
        principal_hash: &str,
        probe_hash: &str,
        probed_at_unix_ms: u64,
    ) -> Self {
        Self {
            host,
            machine_hash: machine_hash.to_owned(),
            principal_hash: principal_hash.to_owned(),
            probe_hash: probe_hash.to_owned(),
            engine_identity_fingerprint: "e".repeat(64),
            probed_at_unix_ms,
            daemon_id: "synthetic-daemon".into(),
            daemon_version: "synthetic-version".into(),
            image_profile: "general".into(),
            image_reference: "ghcr.io/zbarno/koolade-runtime-general:test".into(),
            image_digest: format!("sha256:{}", "a".repeat(64)),
            image_platform: "linux/amd64".into(),
            estimated_pull_bytes: 64 * 1024 * 1024,
            estimated_storage_bytes: 128 * 1024 * 1024,
            disk_available_bytes: Some(4 * 1024 * 1024 * 1024),
            minimum_disk_bytes: 2 * 1024 * 1024 * 1024,
            capacity_probe_required: false,
        }
    }

    pub(super) fn validate(&self, now_ms: u64) -> anyhow::Result<()> {
        anyhow::ensure!(
            is_sha256_hex(&self.machine_hash)
                && is_sha256_hex(&self.principal_hash)
                && is_sha256_hex(&self.probe_hash)
                && is_sha256_hex(&self.engine_identity_fingerprint)
                && !self.daemon_version.is_empty()
                && self.daemon_version.len() <= 128
                && !self
                    .daemon_version
                    .bytes()
                    .any(|byte| byte.is_ascii_control())
                && self.image_digest.starts_with("sha256:")
                && is_sha256_hex(&self.image_digest[7..])
                && matches!(self.image_platform.as_str(), "linux/amd64" | "linux/arm64")
                && matches!(
                    self.image_profile.as_str(),
                    "general" | "rust" | "node" | "python" | "go"
                )
                && self.estimated_pull_bytes > 0
                && self.estimated_storage_bytes >= self.estimated_pull_bytes,
            "Machine approval identity is invalid"
        );
        let required_bytes = self
            .minimum_disk_bytes
            .saturating_add(self.estimated_storage_bytes);
        if self.capacity_probe_required {
            anyhow::ensure!(
                matches!(self.host, Host::MacOs | Host::Windows)
                    && self.image_profile == "general"
                    && self.disk_available_bytes.is_none()
                    && self.estimated_pull_bytes <= DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES
                    && self.estimated_storage_bytes <= DESKTOP_CAPACITY_PROBE_MAX_STORAGE_BYTES,
                "Only Docker Desktop General image preparation may defer VM capacity measurement"
            );
        } else {
            anyhow::ensure!(
                self.disk_available_bytes
                    .is_some_and(|available| available >= required_bytes),
                "Docker storage capacity is unknown or insufficient for the approved image"
            );
        }
        anyhow::ensure!(
            now_ms >= self.probed_at_unix_ms && now_ms - self.probed_at_unix_ms <= SCAN_TTL_MS,
            "Machine readiness evidence is stale; rescan before approving"
        );
        Ok(())
    }

    /// Consent binds whether capacity is measured and its current sample. Probe
    /// time stays outside the digest; Desktop General bootstrap is explicit.
    pub(super) fn consent_digest(&self) -> anyhow::Result<String> {
        fingerprint(&(
            self.host,
            &self.machine_hash,
            &self.principal_hash,
            &self.probe_hash,
            &self.engine_identity_fingerprint,
            &self.daemon_version,
            &self.image_profile,
            self.estimated_pull_bytes,
            self.estimated_storage_bytes,
            self.disk_available_bytes,
            self.minimum_disk_bytes,
            self.capacity_probe_required,
        ))
    }
}

#[allow(dead_code)] // Consumed by the dependent image setup UI in #345.
pub(super) fn current_host() -> Host {
    #[cfg(target_os = "linux")]
    {
        Host::Linux
    }
    #[cfg(target_os = "macos")]
    {
        Host::MacOs
    }
    #[cfg(target_os = "windows")]
    {
        Host::Windows
    }
}

fn fingerprint<T: Serialize>(value: &T) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(value)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(target_os = "linux")]
#[allow(dead_code)] // Consumed by the current host binding derivation in #345.
pub(crate) fn linux_machine_id() -> anyhow::Result<String> {
    use std::{
        fs::OpenOptions,
        io::Read,
        os::unix::{fs::MetadataExt, fs::OpenOptionsExt},
        path::Path,
    };

    let path = Path::new("/etc/machine-id");
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file()
            && metadata.uid() == 0
            && metadata.nlink() == 1
            && metadata.mode() & 0o022 == 0
            && metadata.len() <= 128,
        "OS machine identity is not a trusted regular file"
    );
    let mut value = String::new();
    file.by_ref().take(129).read_to_string(&mut value)?;
    let value = value.trim();
    anyhow::ensure!(
        value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "OS machine identity has an invalid format"
    );
    Ok(value.to_ascii_lowercase())
}

#[cfg(test)]
#[path = "binding/tests.rs"]
mod tests;
