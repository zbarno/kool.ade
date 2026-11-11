//! Separate remote VM worker protocol for Computer Use.
//!
//! This contract is distinct from the Docker execution worker. Remote VM
//! sessions remain unavailable until the independent CU-16 release gate has
//! approved real-worker security and recovery evidence.

mod admission;
#[cfg(target_os = "linux")]
mod backend;
mod enrollment;
mod host_config;
#[cfg(target_os = "linux")]
mod runtime;
mod scheduler;
mod session_store;
mod transport;

pub use admission::{
    ComputerRemoteWorkerAdvertisementV1, ComputerRemoteWorkerRequirementsV1,
    RemoteComputerWorkerEligibilityV1, assess_remote_computer_worker_v1,
};
#[cfg(target_os = "linux")]
pub use backend::RemoteComputerBackendV1;
pub use enrollment::ComputerRemoteWorkerEnrollmentV1;
pub use host_config::{
    ComputerRemoteWorkerHostConfigV1, load_computer_remote_worker_host_config_v1,
};
#[cfg(target_os = "linux")]
pub use runtime::{
    ComputerRemoteWorkerOverlayProviderV1, LocalKvmComputerRemoteWorkerRuntimeV1,
    SignedCatalogRemoteWorkerOverlayProviderV1,
};
pub use scheduler::{ComputerRemoteWorkerSchedulerV1, RemoteComputerWorkerPermitV1};
pub use session_store::{ComputerRemoteWorkerRecoveryRecordV1, ComputerRemoteWorkerSessionStoreV1};
pub use transport::{
    ComputerRemoteWorkerCheckpointStoreV1, ComputerRemoteWorkerCheckpointV1,
    ComputerRemoteWorkerClientV1, ComputerRemoteWorkerOperationV1,
    ComputerRemoteWorkerRequestBodyV1, ComputerRemoteWorkerRequestV1,
    ComputerRemoteWorkerResponseV1, ComputerRemoteWorkerResultV1, ComputerRemoteWorkerRuntimeV1,
    ComputerRemoteWorkerSessionFenceV1, FileComputerRemoteWorkerCheckpointStoreV1,
    MAX_REMOTE_COMPUTER_WORKER_FRAME_BYTES_V1, decode_remote_computer_worker_frame_v1,
    encode_remote_computer_worker_frame_v1, serve_computer_remote_worker_stdio_v1,
};

/// Start one explicitly configured Linux worker stream. Every state and image
/// path is supplied by the operator; this function has no implicit defaults.
#[cfg(target_os = "linux")]
pub fn serve_computer_use_worker_stdio_v1(config_path: &std::path::Path) -> anyhow::Result<()> {
    use crate::computer_use::{ComputerBackend, ComputerResourceLimitsV1, LocalKvmComputerBackend};
    use std::io;

    let config = load_computer_remote_worker_host_config_v1(config_path)?;
    let controller_key = config.validate().map_err(anyhow::Error::msg)?;
    let limits = ComputerResourceLimitsV1 {
        max_vcpus: config.max_vcpus,
        max_memory_bytes: config.max_memory_bytes,
        max_overlay_bytes: config.max_disk_bytes,
        max_processes: 32,
        parallel_slots_per_machine: config.max_parallel_sessions,
        boot_timeout: std::time::Duration::from_secs(120),
        stop_timeout: std::time::Duration::from_secs(10),
    };
    let backend = std::sync::Arc::new(
        LocalKvmComputerBackend::new_for_remote_worker(
            config.runtime_root.clone(),
            config.overlay_root.clone(),
            config.cgroup_root.clone(),
            config.machine_id.clone(),
            config.worker_id.clone(),
            config.controller_key_id.clone(),
            controller_key,
            limits,
        )
        .map_err(|error| anyhow::anyhow!("remote worker backend refused startup: {error}"))?,
    );
    backend
        .probe()
        .map_err(|error| anyhow::anyhow!("remote worker host is not ready: {error}"))?;
    let overlays = SignedCatalogRemoteWorkerOverlayProviderV1::open(
        config.image_cache_root.clone(),
        &config.catalog_path,
        config.generation_floor_root.clone(),
        config.overlay_root.clone(),
        config.controller_key_id.clone(),
        controller_key,
    )?;
    let runtime = LocalKvmComputerRemoteWorkerRuntimeV1::new(config.clone(), backend, overlays)?;
    let store = FileComputerRemoteWorkerCheckpointStoreV1::open(&config.checkpoint_path)?;
    serve_computer_remote_worker_stdio_v1(io::stdin(), io::stdout(), store, runtime, &config)?;
    Ok(())
}

/// Product entry points must check this gate before advertising or allocating
/// remote VM sessions. It stays closed until CU-16 accepts real-worker tests.
pub const fn remote_computer_use_release_gate_open_v1() -> bool {
    false
}

/// Explicit operator recovery command. It creates a strictly generation-
/// advancing signed replacement and never replays an uncertain guest action.
#[cfg(not(target_os = "linux"))]
pub fn recover_computer_use_worker_session_v1(
    _worker_id: &str,
    _session_id: &str,
    _explicitly_approved: bool,
) -> anyhow::Result<bool> {
    anyhow::bail!("remote Computer Use recovery requires Linux with supported KVM isolation")
}

#[cfg(target_os = "linux")]
pub fn recover_computer_use_worker_session_v1(
    worker_id: &str,
    session_id: &str,
    explicitly_approved: bool,
) -> anyhow::Result<bool> {
    anyhow::ensure!(
        explicitly_approved,
        "recovery requires explicit operator approval"
    );
    anyhow::ensure!(
        !remote_computer_use_release_gate_open_v1(),
        "remote worker release certification is required before recovery"
    );
    let enrollment = crate::persistence::computer_use_remote_workers::get(worker_id)?
        .ok_or_else(|| anyhow::anyhow!("Computer Use worker is not enrolled"))?;
    let selected = crate::persistence::computer_use_remote_workers::selected()?
        .ok_or_else(|| anyhow::anyhow!("no Computer Use worker is selected"))?;
    anyhow::ensure!(
        selected.worker_id == worker_id,
        "recovery worker is not the explicitly selected target"
    );
    let storage = crate::persistence::setup_tasks::computer_use_storage_dir()?;
    let journal_root = storage.join("remote-worker-controller");
    let store = ComputerRemoteWorkerSessionStoreV1::open(journal_root.join("session-recovery"))?;
    let mut record = store
        .load(session_id)?
        .ok_or_else(|| anyhow::anyhow!("remote session recovery intent was not found"))?;
    anyhow::ensure!(
        record.worker_id == worker_id,
        "recovery record belongs to a different worker"
    );
    let (controller_key_id, signing_key) = crate::persistence::computer_use_controller_key::load_or_create_computer_use_controller_key_v1()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis()
        .min(u64::MAX as u128) as u64;
    let mut replacement = record.session.clone();
    replacement.lease.generation = replacement
        .lease
        .generation
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("lease generation exhausted"))?;
    replacement.cancellation_generation = replacement
        .cancellation_generation
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("cancellation generation exhausted"))?;
    replacement.lease.cancellation_generation = replacement.cancellation_generation;
    replacement.approval_generation = replacement
        .approval_generation
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("approval generation exhausted"))?;
    replacement.policy.approval_generation = replacement.approval_generation;
    replacement.session_started_at_unix_ms = now;
    replacement.lease.issued_at_unix_ms = now;
    replacement.lease.expires_at_unix_ms = now.saturating_add(
        replacement
            .policy
            .resource_budget
            .time_limit_ms
            .min(15 * 60 * 1_000),
    );
    replacement
        .attest(controller_key_id.clone(), &signing_key)
        .map_err(anyhow::Error::msg)?;
    let backend = RemoteComputerBackendV1::new(
        enrollment,
        std::path::PathBuf::new(),
        std::path::PathBuf::new(),
        controller_key_id,
        signing_key.verifying_key(),
        journal_root,
    )?;
    let last_event_sequence = record.last_event_sequence;
    backend.recover_session(&replacement, last_event_sequence, now)?;
    record.session = replacement;
    Ok(record.pending_action_sequence.is_some())
}
