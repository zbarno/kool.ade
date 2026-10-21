use serde::{Deserialize, Serialize};

use super::{RemoteWorkerRegistration, load_from, registration_path};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum RemoteWorkerHealth {
    Healthy,
    Busy,
    Disconnected,
    Untrusted,
    Incompatible,
    PolicyBlocked,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RemoteWorkerProbe {
    pub observed_at_unix_ms: u64,
    pub health: RemoteWorkerHealth,
    pub architecture: Option<String>,
    pub running_jobs: u32,
    pub policy_revision: String,
    #[serde(default)]
    pub docker_version: Option<String>,
    #[serde(default)]
    pub trusted_image_digests: Vec<String>,
    #[serde(default)]
    pub resource_ceiling: Option<crate::execution::contract::ResourceBudgetV1>,
    #[serde(default)]
    pub disk_available_bytes: Option<u64>,
    #[serde(default)]
    pub available_cpu_millis: Option<u32>,
    #[serde(default)]
    pub available_memory_bytes: Option<u64>,
    #[serde(default)]
    pub cpu_pressure_some_avg10_basis_points: Option<u32>,
    #[serde(default)]
    pub load_millis: Option<u32>,
    /// Available to app-owned execution containers; this does not authorize
    /// arbitrary host device access.
    #[serde(default)]
    pub gpu_available: Option<bool>,
    /// Available inside an app-owned isolated runtime; host display sockets
    /// are never forwarded to task containers.
    #[serde(default)]
    pub display_available: Option<bool>,
}

/// Record only public health/capability facts; never persist host names, SSH
/// configuration, credentials, or project data in shared evidence.
pub fn record_probe(worker_id: &str, mut probe: RemoteWorkerProbe) -> anyhow::Result<()> {
    let root = crate::persistence::state_root();
    let mut registration: RemoteWorkerRegistration = load_from(&root, worker_id)?
        .ok_or_else(|| anyhow::anyhow!("remote worker is not enrolled"))?;
    probe.observed_at_unix_ms = crate::persistence::harness_settings::current_unix_ms();
    registration.last_probe = Some(probe.clone());
    registration.policy_revision = probe.policy_revision;
    registration.validate()?;
    let path = registration_path(&root, worker_id)?;
    crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec(&registration)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
