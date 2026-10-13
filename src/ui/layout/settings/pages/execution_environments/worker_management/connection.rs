use crate::{
    execution::remote::{client::RemoteWorkerClient, ssh::SshWorkerConfig},
    persistence::remote_workers::{self, RemoteWorkerRegistration},
};
use std::path::PathBuf;

pub(super) fn test_connection(worker: &RemoteWorkerRegistration) -> Result<String, String> {
    let config = worker_config(worker).map_err(|error| error.to_string())?;
    let client = RemoteWorkerClient::connect_for_probe(&config).map_err(|error| error.message)?;
    let probe = client.probe();
    let capabilities = probe.capabilities.as_ref();
    Ok(format!(
        "Passed: {} is compatible; architecture {}, Docker {}, {} active tasks, policy {}, approved images {}.",
        worker.worker_id,
        probe.architecture.as_deref().unwrap_or("unknown"),
        capabilities
            .and_then(|value| value.docker_version.as_deref())
            .unwrap_or("unknown"),
        capabilities.map_or(0, |value| value.current_execution_count),
        capabilities.map_or("unknown", |value| value.policy_hash.as_str()),
        capabilities.map_or(0, |value| value.trusted_image_digests.len())
    ))
}

pub(super) fn trust_live_capabilities(worker: &RemoteWorkerRegistration) -> Result<String, String> {
    let config = worker_config(worker).map_err(|error| error.to_string())?;
    let client = RemoteWorkerClient::connect_for_probe(&config).map_err(|error| error.message)?;
    let updated =
        remote_workers::trust_reported_capabilities(worker.worker_id.as_str(), client.probe())
            .map_err(|error| error.to_string())?;
    Ok(format!(
        "Passed: trusted {} locally approved image digest(s) for {}.",
        updated.trusted_image_digests.len(),
        worker.worker_id
    ))
}

fn worker_config(worker: &RemoteWorkerRegistration) -> anyhow::Result<SshWorkerConfig> {
    worker.validate()?;
    let known_hosts_path = crate::persistence::state_root()
        .join("remote-workers")
        .join("known_hosts");
    let metadata = std::fs::symlink_metadata(&known_hosts_path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "approved worker known_hosts file is unsafe"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        anyhow::ensure!(
            metadata.permissions().mode() & 0o077 == 0,
            "approved worker known_hosts file must be private"
        );
    }
    Ok(SshWorkerConfig {
        stable_id: worker.worker_id.clone(),
        host_alias: worker.ssh_host_alias.clone(),
        ssh_principal: worker.ssh_principal.clone(),
        verified_host_key_fingerprint: worker.host_key_fingerprint.clone(),
        known_hosts_path,
        ssh_binary: PathBuf::from("ssh"),
    })
}
