//! Operator-local authenticated remote-worker registrations.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const DIRECTORY: &str = "remote-workers";

mod capacity;
mod config;
mod enrollment;
mod probe;
pub use capacity::{
    AutoWorkerCapacityCandidate, RemoteWorkerCapacityLease, order_auto_workers,
    record_auto_worker_selection, reserve_auto_capacity, reserve_capacity,
};
pub(crate) use config::trust_reported_capabilities;
pub use config::{configure, configure_resource_limit, list};
pub use enrollment::{revoke, trust};
pub use probe::{RemoteWorkerHealth, RemoteWorkerProbe, record_probe};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RemoteWorkerRegistration {
    pub schema_version: u8,
    pub worker_id: String,
    pub ssh_host_alias: String,
    pub ssh_principal: String,
    pub host_key_fingerprint: String,
    pub enrolled_at_unix_ms: u64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_jobs")]
    pub max_jobs: u32,
    #[serde(default)]
    pub supported_architectures: Vec<String>,
    #[serde(default)]
    pub trusted_image_digests: Vec<String>,
    #[serde(default)]
    pub resource_limit: Option<crate::execution::contract::ResourceBudgetV1>,
    #[serde(default)]
    pub last_probe: Option<RemoteWorkerProbe>,
    #[serde(default)]
    pub policy_revision: String,
}

impl RemoteWorkerRegistration {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported worker registration version"
        );
        anyhow::ensure!(valid_identifier(&self.worker_id), "worker ID is invalid");
        anyhow::ensure!(
            valid_host_token(&self.ssh_host_alias),
            "SSH host alias is invalid"
        );
        anyhow::ensure!(
            valid_host_token(&self.ssh_principal),
            "SSH principal is invalid"
        );
        anyhow::ensure!(
            self.host_key_fingerprint.starts_with("SHA256:")
                && self.host_key_fingerprint.len() == 50
                && self.host_key_fingerprint[7..]
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/')),
            "worker host-key fingerprint is invalid"
        );
        anyhow::ensure!(
            self.enrolled_at_unix_ms > 0,
            "worker enrollment time is invalid"
        );
        anyhow::ensure!(
            (1..=256).contains(&self.max_jobs),
            "worker job limit is invalid"
        );
        anyhow::ensure!(
            self.supported_architectures
                .iter()
                .all(|architecture| matches!(architecture.as_str(), "amd64" | "arm64")),
            "worker architecture list is invalid"
        );
        anyhow::ensure!(
            self.trusted_image_digests
                .iter()
                .all(|digest| valid_image_digest(digest)),
            "worker image digest list is invalid"
        );
        if let Some(limit) = &self.resource_limit {
            limit.validate()?;
        }
        if let Some(probe) = &self.last_probe {
            anyhow::ensure!(
                probe.observed_at_unix_ms > 0,
                "worker probe time is invalid"
            );
            anyhow::ensure!(probe.running_jobs <= 1_000_000, "worker load is invalid");
            anyhow::ensure!(
                probe
                    .trusted_image_digests
                    .iter()
                    .all(|digest| valid_image_digest(digest)),
                "worker probe contains an invalid image digest"
            );
            anyhow::ensure!(
                probe
                    .docker_version
                    .as_ref()
                    .is_none_or(|version| { !version.trim().is_empty() && version.len() <= 128 }),
                "worker probe contains an invalid Docker version"
            );
            if let Some(ceiling) = &probe.resource_ceiling {
                ceiling.validate()?;
            }
            anyhow::ensure!(
                probe
                    .architecture
                    .as_deref()
                    .is_none_or(|architecture| matches!(architecture, "amd64" | "arm64")),
                "worker probe architecture is invalid"
            );
        }
        Ok(())
    }
}

/// Enroll an operator-approved SSH host identity. Existing IDs cannot be
/// silently rebound; a changed host requires an explicit local state reset.
pub fn register(
    worker_id: &str,
    ssh_host_alias: &str,
    ssh_principal: &str,
    host_key_fingerprint: &str,
) -> anyhow::Result<RemoteWorkerRegistration> {
    let registration = RemoteWorkerRegistration {
        schema_version: 1,
        worker_id: worker_id.to_owned(),
        ssh_host_alias: ssh_host_alias.to_owned(),
        ssh_principal: ssh_principal.to_owned(),
        host_key_fingerprint: host_key_fingerprint.to_owned(),
        enrolled_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
        enabled: true,
        max_jobs: 1,
        supported_architectures: Vec::new(),
        trusted_image_digests: Vec::new(),
        resource_limit: None,
        last_probe: None,
        policy_revision: String::new(),
    };
    registration.validate()?;
    register_at(&crate::persistence::state_root(), registration)
}

fn register_at(
    root: &Path,
    registration: RemoteWorkerRegistration,
) -> anyhow::Result<RemoteWorkerRegistration> {
    registration.validate()?;
    let path = registration_path(root, &registration.worker_id)?;
    create_private_dir(path.parent().expect("registration path has parent"))?;
    let bytes = serde_json::to_vec(&registration)?;
    match crate::artifacts::atomic_create_bytes(&path, &bytes) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            }
            Ok(registration)
        }
        Err(_) if path.exists() => {
            let existing = load_from(root, &registration.worker_id)?
                .ok_or_else(|| anyhow::anyhow!("existing worker registration disappeared"))?;
            anyhow::ensure!(
                same_binding(&existing, &registration),
                "worker ID is already bound to a different SSH identity"
            );
            Ok(existing)
        }
        Err(error) => Err(error.into()),
    }
}

pub fn get(worker_id: &str) -> anyhow::Result<Option<RemoteWorkerRegistration>> {
    load_from(&crate::persistence::state_root(), worker_id)
}

fn load_from(root: &Path, worker_id: &str) -> anyhow::Result<Option<RemoteWorkerRegistration>> {
    let path = registration_path(root, worker_id)?;
    let file = match open_registration(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(16 * 1024 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 16 * 1024, "worker registration is too large");
    let registration: RemoteWorkerRegistration = serde_json::from_slice(&bytes)?;
    registration.validate()?;
    anyhow::ensure!(
        registration.worker_id == worker_id,
        "worker registration identity mismatch"
    );
    Ok(Some(registration))
}

fn registration_path(root: &Path, worker_id: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(valid_identifier(worker_id), "worker ID is invalid");
    Ok(root.join(DIRECTORY).join(format!("{worker_id}.json")))
}

fn create_private_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "worker registry directory is not trusted"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn open_registration(path: &Path) -> std::io::Result<fs::File> {
    use std::io;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "worker registration is not a regular file",
        ));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_image_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn default_enabled() -> bool {
    true
}

fn default_max_jobs() -> u32 {
    1
}

fn same_binding(left: &RemoteWorkerRegistration, right: &RemoteWorkerRegistration) -> bool {
    left.worker_id == right.worker_id
        && left.ssh_host_alias == right.ssh_host_alias
        && left.ssh_principal == right.ssh_principal
        && left.host_key_fingerprint == right.host_key_fingerprint
}

fn valid_host_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
#[path = "remote_workers/tests.rs"]
mod tests;
