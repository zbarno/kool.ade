use crate::{
    execution::{
        contract::{ExecutionModeV1, ExecutionTarget, ResourceBudgetV1},
        docker::images::{ImageLock, RuntimeProfile},
        remote::{client::RemoteWorkerClient, scheduler::MAX_HEALTH_AGE_MS, ssh::SshWorkerConfig},
        session::ExecutionSession,
    },
    harness::PlanningRequest,
    persistence::remote_workers::{self, RemoteWorkerRegistration, reserve_capacity},
};
use std::path::PathBuf;

pub(super) fn remote_session(
    request: &PlanningRequest,
    workspace: crate::execution::contract::WorkspaceRef,
    profile: RuntimeProfile,
    resources: ResourceBudgetV1,
    worker_id: &str,
) -> anyhow::Result<ExecutionSession> {
    let verifier = request
        .authorization_verifier
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Remote execution requires an active task claim"))?;
    verifier
        .verify(&verifier.generation())
        .map_err(|error| anyhow::anyhow!("Remote task claim is unavailable: {}", error.message))?;
    let registration = remote_workers::get(worker_id)?
        .ok_or_else(|| anyhow::anyhow!("Selected remote worker is not enrolled"))?;
    anyhow::ensure!(registration.enabled, "Selected remote worker is disabled");
    let config = worker_config(&registration)?;
    let observer = RemoteWorkerClient::connect_for_probe(&config).map_err(|error| {
        anyhow::anyhow!(
            "Selected remote worker failed host-key/capability verification: {}",
            error.message
        )
    })?;
    let probe = observer.probe();
    let registration = remote_workers::get(worker_id)?
        .ok_or_else(|| anyhow::anyhow!("Selected remote worker registration disappeared"))?;
    verify_registration_capabilities(&registration, probe, profile, &resources)?;
    let running_jobs = probe
        .capabilities
        .as_ref()
        .map(|capabilities| capabilities.current_execution_count)
        .ok_or_else(|| anyhow::anyhow!("Remote worker did not report execution capacity"))?;
    let worker_capacity = probe
        .capabilities
        .as_ref()
        .and_then(|capabilities| capabilities.maximum_execution_count)
        .map_or(registration.max_jobs, |maximum| {
            maximum.min(registration.max_jobs)
        });
    let capacity = reserve_capacity(
        &registration.worker_id,
        &registration.host_key_fingerprint,
        worker_capacity,
        running_jobs,
    )?;
    drop(observer);
    let target = ExecutionTarget::remote(worker_id.to_owned());
    let session = ExecutionSession::connect_remote_with_task_clone(
        workspace,
        crate::execution::session::RemoteSessionOptions {
            target,
            worker: &config,
            mode: ExecutionModeV1::Implementation,
            profile,
            resources,
            authorization_verifier: Some(verifier),
            task_clone: Some(&request.repo_root),
        },
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "Selected remote worker failed live verification: {}",
            error.message
        )
    })?;
    session
        .retain_remote_capacity_lease(capacity)
        .map_err(|error| {
            anyhow::anyhow!("Could not retain remote worker capacity: {}", error.message)
        })?;
    Ok(session)
}

fn verify_registration_capabilities(
    registration: &RemoteWorkerRegistration,
    probe: &crate::execution::backend::BackendProbeV1,
    profile: RuntimeProfile,
    resources: &ResourceBudgetV1,
) -> anyhow::Result<()> {
    let capabilities = probe
        .capabilities
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Remote worker did not report capabilities"))?;
    let effective_max = capabilities
        .maximum_execution_count
        .map_or(registration.max_jobs, |maximum| {
            maximum.min(registration.max_jobs)
        });
    let architecture = probe
        .architecture
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Remote worker architecture is unavailable"))?;
    let image = ImageLock::bundled()
        .map_err(|failure| anyhow::anyhow!(failure.message))?
        .select(profile, architecture)
        .map_err(|failure| anyhow::anyhow!(failure.message))?;
    anyhow::ensure!(
        registration
            .supported_architectures
            .iter()
            .any(|value| value == architecture),
        "Remote worker architecture is not explicitly enabled"
    );
    anyhow::ensure!(
        registration
            .trusted_image_digests
            .iter()
            .any(|value| value == &image.digest),
        "Remote worker image digest is not explicitly trusted"
    );
    anyhow::ensure!(
        capabilities
            .trusted_image_digests
            .iter()
            .any(|value| value == &image.digest),
        "Remote worker does not trust the selected executor image"
    );
    anyhow::ensure!(
        capabilities.policy_hash == crate::execution::policy::policy_hash()
            && registration.policy_revision == capabilities.policy_hash,
        "Remote worker execution policy differs from this controller"
    );
    anyhow::ensure!(
        capabilities.current_execution_count < effective_max,
        "Remote worker has reached its configured concurrency limit"
    );
    anyhow::ensure!(
        capabilities
            .disk_available_bytes
            .is_some_and(|value| value >= resources.temporary_disk_bytes),
        "Remote worker does not have enough temporary disk space"
    );
    let ceiling = &capabilities.resource_ceiling;
    if let Some(limit) = registration.resource_limit.as_ref() {
        anyhow::ensure!(
            resources.cpu_millis <= limit.cpu_millis
                && resources.memory_bytes <= limit.memory_bytes
                && resources.process_count <= limit.process_count
                && resources.temporary_disk_bytes <= limit.temporary_disk_bytes
                && resources.output_bytes <= limit.output_bytes,
            "Requested execution exceeds the worker's configured resource limit"
        );
    }
    anyhow::ensure!(
        resources.cpu_millis <= ceiling.cpu_millis
            && resources.memory_bytes <= ceiling.memory_bytes
            && resources.process_count <= ceiling.process_count
            && resources.temporary_disk_bytes <= ceiling.temporary_disk_bytes
            && resources.output_bytes <= ceiling.output_bytes,
        "Remote worker resource ceiling is too small for this execution"
    );
    let now = crate::persistence::harness_settings::current_unix_ms();
    let latest = registration
        .last_probe
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Remote worker has no recorded health probe"))?;
    anyhow::ensure!(
        latest.observed_at_unix_ms <= now
            && now.saturating_sub(latest.observed_at_unix_ms) <= MAX_HEALTH_AGE_MS,
        "Remote worker health probe is stale"
    );
    anyhow::ensure!(
        matches!(latest.health, remote_workers::RemoteWorkerHealth::Healthy),
        "Remote worker is not healthy"
    );
    Ok(())
}

pub(super) fn worker_config(
    registration: &RemoteWorkerRegistration,
) -> anyhow::Result<SshWorkerConfig> {
    registration.validate()?;
    let state_root = crate::persistence::state_root();
    let known_hosts_path = state_root.join("remote-workers").join("known_hosts");
    let metadata = std::fs::symlink_metadata(&known_hosts_path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "approved worker known_hosts file is unsafe"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        anyhow::ensure!(
            metadata.uid() == unsafe { geteuid() } && metadata.permissions().mode() & 0o077 == 0,
            "approved worker known_hosts file must be private to the current user"
        );
    }
    Ok(SshWorkerConfig {
        stable_id: registration.worker_id.clone(),
        host_alias: registration.ssh_host_alias.clone(),
        ssh_principal: registration.ssh_principal.clone(),
        verified_host_key_fingerprint: registration.host_key_fingerprint.clone(),
        known_hosts_path,
        ssh_binary: PathBuf::from("ssh"),
    })
}

#[cfg(unix)]
unsafe extern "C" {
    fn geteuid() -> u32;
}
