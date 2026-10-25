use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::app::bootstrap_repair_policy::{Host, Proposal};

const SCAN_TTL_MS: u64 = 5 * 60 * 1000;

mod engine;
pub(super) use engine::VerifiedEngine;

#[derive(Clone, Debug, Serialize)]
pub(super) struct MachineBinding {
    pub(super) host: Host,
    pub(super) machine_hash: String,
    pub(super) principal_hash: String,
    pub(super) probe_hash: String,
    pub(super) probed_at_unix_ms: u64,
    pub(super) daemon_id: String,
    pub(super) image_reference: String,
    pub(super) image_digest: String,
    pub(super) image_platform: String,
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
        Ok(Self {
            host,
            machine_hash,
            principal_hash,
            probe_hash: probe_hash.to_owned(),
            probed_at_unix_ms,
            daemon_id: "synthetic-daemon".into(),
            image_reference: "ghcr.io/zbarno/koolade-runtime-general:test".into(),
            image_digest: format!("sha256:{}", "a".repeat(64)),
            image_platform: "linux/amd64".into(),
        })
    }

    pub(super) fn validate(&self, now_ms: u64) -> anyhow::Result<()> {
        anyhow::ensure!(
            is_sha256_hex(&self.machine_hash)
                && is_sha256_hex(&self.principal_hash)
                && is_sha256_hex(&self.probe_hash)
                && self.image_digest.starts_with("sha256:")
                && is_sha256_hex(&self.image_digest[7..])
                && matches!(self.image_platform.as_str(), "linux/amd64" | "linux/arm64"),
            "Machine approval identity is invalid"
        );
        anyhow::ensure!(
            now_ms >= self.probed_at_unix_ms && now_ms - self.probed_at_unix_ms <= SCAN_TTL_MS,
            "Machine readiness evidence is stale; rescan before approving"
        );
        Ok(())
    }

    /// Identity digest excludes probe time; freshness is checked separately.
    pub(super) fn consent_digest(&self) -> anyhow::Result<String> {
        fingerprint(&(
            self.host,
            &self.machine_hash,
            &self.principal_hash,
            &self.probe_hash,
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
mod tests {
    use super::*;

    #[test]
    fn machine_and_principal_hashes_are_derived_and_domain_separated() {
        let first =
            MachineBinding::from_facts(Host::Linux, "machine-one", "1000", &"a".repeat(64), 10)
                .unwrap();
        let other_principal =
            MachineBinding::from_facts(Host::Linux, "machine-one", "1001", &"a".repeat(64), 10)
                .unwrap();
        let other_machine =
            MachineBinding::from_facts(Host::Linux, "machine-two", "1000", &"a".repeat(64), 10)
                .unwrap();
        assert_ne!(first.machine_hash, other_machine.machine_hash);
        assert_ne!(first.principal_hash, other_principal.principal_hash);
        first.validate(11).unwrap();
        assert!(first.validate(10 + SCAN_TTL_MS + 1).is_err());
    }

    #[test]
    fn endpoint_and_probe_hash_must_be_sha256() {
        let binding = MachineBinding::from_facts(Host::Linux, "id", "1", "not-a-hash", 10).unwrap();
        assert!(binding.validate(11).is_err());
    }
}
