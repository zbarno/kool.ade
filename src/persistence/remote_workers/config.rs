use super::{RemoteWorkerRegistration, load_from, registration_path};
use std::fs;

/// Apply explicit operator constraints to an enrolled worker registration.
pub fn configure(
    worker_id: &str,
    enabled: bool,
    max_jobs: u32,
    supported_architectures: Vec<String>,
    trusted_image_digests: Vec<String>,
) -> anyhow::Result<RemoteWorkerRegistration> {
    let root = crate::persistence::state_root();
    let mut registration = load_from(&root, worker_id)?
        .ok_or_else(|| anyhow::anyhow!("remote worker is not enrolled"))?;
    registration.enabled = enabled;
    registration.max_jobs = max_jobs;
    registration.supported_architectures = supported_architectures;
    registration.trusted_image_digests = trusted_image_digests;
    registration.validate()?;
    let path = registration_path(&root, worker_id)?;
    crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec(&registration)?)?;
    set_private(path)?;
    Ok(registration)
}

pub fn configure_resource_limit(
    worker_id: &str,
    resource_limit: Option<crate::execution::contract::ResourceBudgetV1>,
) -> anyhow::Result<RemoteWorkerRegistration> {
    let root = crate::persistence::state_root();
    let mut registration = load_from(&root, worker_id)?
        .ok_or_else(|| anyhow::anyhow!("remote worker is not enrolled"))?;
    if let Some(limit) = &resource_limit {
        let ceiling = registration
            .last_probe
            .as_ref()
            .and_then(|probe| probe.resource_ceiling.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Test the worker connection before setting limits"))?;
        ensure_within_resource_ceiling(limit, ceiling)?;
    }
    registration.resource_limit = resource_limit;
    registration.validate()?;
    let path = registration_path(&root, worker_id)?;
    crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec(&registration)?)?;
    set_private(path)?;
    Ok(registration)
}

fn ensure_within_resource_ceiling(
    limit: &crate::execution::contract::ResourceBudgetV1,
    ceiling: &crate::execution::contract::ResourceBudgetV1,
) -> anyhow::Result<()> {
    limit.validate()?;
    anyhow::ensure!(
        limit.cpu_millis <= ceiling.cpu_millis
            && limit.memory_bytes <= ceiling.memory_bytes
            && limit.process_count <= ceiling.process_count
            && limit.temporary_disk_bytes <= ceiling.temporary_disk_bytes
            && limit.output_bytes <= ceiling.output_bytes,
        "Worker resource limit exceeds its last reported capability ceiling"
    );
    Ok(())
}

/// Explicitly allow only the current app's bundled executor digests that a
/// healthy, identity-verified probe observed on this worker.
pub(crate) fn trust_reported_capabilities(
    worker_id: &str,
    probe: &crate::execution::backend::BackendProbeV1,
) -> anyhow::Result<RemoteWorkerRegistration> {
    let root = crate::persistence::state_root();
    let mut registration = load_from(&root, worker_id)?
        .ok_or_else(|| anyhow::anyhow!("remote worker is not enrolled"))?;
    anyhow::ensure!(
        probe.available && probe.compatible && probe.failure.is_none(),
        "Only a successful, identity-verified live probe can report trusted runtimes"
    );
    let capabilities = probe
        .capabilities
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Worker did not report runtime capabilities"))?;
    anyhow::ensure!(
        capabilities.app_version == env!("CARGO_PKG_VERSION")
            && capabilities.policy_hash == crate::execution::policy::policy_hash(),
        "Worker policy differs from the local execution policy"
    );
    let architecture = probe
        .architecture
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Worker architecture is unavailable"))?;
    let lock = crate::execution::docker::images::ImageLock::bundled()
        .map_err(|failure| anyhow::anyhow!(failure.message))?;
    let approved =
        locally_approved_reported_digests(&lock, architecture, &capabilities.trusted_image_digests);
    anyhow::ensure!(
        !approved.is_empty(),
        "Worker reported no locally approved executor image digests"
    );
    registration.supported_architectures = vec![architecture.to_owned()];
    registration.trusted_image_digests = approved;
    registration.validate()?;
    let path = registration_path(&root, worker_id)?;
    crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec(&registration)?)?;
    set_private(path)?;
    Ok(registration)
}

fn locally_approved_reported_digests(
    lock: &crate::execution::docker::images::ImageLock,
    architecture: &str,
    reported_digests: &[String],
) -> Vec<String> {
    let allowed = lock
        .profiles
        .values()
        .flatten()
        .filter(|image| image.runtime_enabled && image.platform == format!("linux/{architecture}"))
        .map(|image| image.digest.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let reported = reported_digests
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    allowed
        .intersection(&reported)
        .copied()
        .map(str::to_owned)
        .collect()
}

pub fn list() -> anyhow::Result<Vec<RemoteWorkerRegistration>> {
    let state_root = crate::persistence::state_root();
    let root = state_root.join("remote-workers");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut registrations = Vec::new();
    for entry in entries {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "worker registry contains an unsafe entry"
        );
        if entry
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("json")
        {
            continue;
        }
        let path = entry.path();
        let worker_id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| anyhow::anyhow!("worker registration name is invalid"))?;
        registrations.push(
            load_from(&state_root, worker_id)?
                .ok_or_else(|| anyhow::anyhow!("worker registration disappeared"))?,
        );
    }
    registrations.sort_by(|left, right| left.worker_id.cmp(&right.worker_id));
    Ok(registrations)
}

fn set_private(path: std::path::PathBuf) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "config/tests.rs"]
mod tests;
