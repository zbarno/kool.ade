//! Deterministic, fail-closed target selection for explicit remote workers.

use crate::execution::{
    backend::BackendCapabilitiesV1,
    contract::{ExecutionTarget, ResourceBudgetV1, TargetPreference},
};
use crate::persistence::remote_workers::{RemoteWorkerHealth, RemoteWorkerRegistration};

pub const MAX_HEALTH_AGE_MS: u64 = 45_000;

pub struct WorkerCandidate<'a> {
    pub registration: &'a RemoteWorkerRegistration,
    pub capabilities: &'a BackendCapabilitiesV1,
}

pub struct SchedulingRequirements<'a> {
    pub architecture: &'a str,
    pub image_digest: &'a str,
    pub policy_revision: &'a str,
    pub resource_budget: &'a ResourceBudgetV1,
    pub remote_approved: bool,
    pub dependencies_prepared: bool,
    pub eligible_worker_ids: &'a [String],
    pub requires_gpu: bool,
    pub requires_display: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionFailure {
    NoRemoteApproval,
    WorkerUnavailable,
    NoCompatibleTarget,
}

pub fn select_target(
    preference: &TargetPreference,
    local_eligible: bool,
    candidates: &[WorkerCandidate<'_>],
    requirements: &SchedulingRequirements<'_>,
    now_unix_ms: u64,
) -> Result<ExecutionTarget, SelectionFailure> {
    match preference {
        TargetPreference::Local => local_target_eligible(local_eligible, requirements)
            .then(ExecutionTarget::local)
            .ok_or(SelectionFailure::NoCompatibleTarget),
        TargetPreference::Worker(worker_id) => {
            if !requirements.remote_approved {
                return Err(SelectionFailure::NoRemoteApproval);
            }
            if !requirements.eligible_worker_ids.is_empty()
                && !requirements
                    .eligible_worker_ids
                    .iter()
                    .any(|id| id == worker_id)
            {
                return Err(SelectionFailure::WorkerUnavailable);
            }
            let candidate = candidates
                .iter()
                .find(|candidate| candidate.registration.worker_id == *worker_id)
                .ok_or(SelectionFailure::WorkerUnavailable)?;
            candidate_eligible(candidate, requirements, now_unix_ms)
                .then(|| ExecutionTarget::remote(worker_id.clone()))
                .ok_or(SelectionFailure::NoCompatibleTarget)
        }
        TargetPreference::Auto => {
            let worker_id = if requirements.remote_approved && requirements.dependencies_prepared {
                candidates
                    .iter()
                    .filter(|candidate| {
                        (requirements.eligible_worker_ids.is_empty()
                            || requirements
                                .eligible_worker_ids
                                .iter()
                                .any(|id| id == &candidate.registration.worker_id))
                            && candidate_eligible(candidate, requirements, now_unix_ms)
                    })
                    .min_by_key(|candidate| {
                        (
                            std::cmp::Reverse(
                                candidate
                                    .capabilities
                                    .available_cpu_millis
                                    .unwrap_or_default(),
                            ),
                            std::cmp::Reverse(
                                candidate
                                    .capabilities
                                    .available_memory_bytes
                                    .unwrap_or_default(),
                            ),
                            candidate
                                .capabilities
                                .cpu_pressure_some_avg10_basis_points
                                .unwrap_or(u32::MAX),
                            candidate.capabilities.load_millis.unwrap_or(u32::MAX),
                            candidate
                                .registration
                                .last_probe
                                .as_ref()
                                .map_or(u32::MAX, |p| p.running_jobs),
                            requirements
                                .eligible_worker_ids
                                .iter()
                                .position(|id| id == &candidate.registration.worker_id)
                                .unwrap_or(usize::MAX),
                            candidate.registration.worker_id.as_str(),
                        )
                    })
                    .map(|candidate| candidate.registration.worker_id.clone())
            } else {
                None
            };
            if let Some(worker_id) = worker_id {
                Ok(ExecutionTarget::remote(worker_id))
            } else if local_target_eligible(local_eligible, requirements) {
                Ok(ExecutionTarget::local())
            } else if !requirements.remote_approved {
                Err(SelectionFailure::NoRemoteApproval)
            } else {
                Err(SelectionFailure::NoCompatibleTarget)
            }
        }
    }
}

fn local_target_eligible(local_eligible: bool, requirements: &SchedulingRequirements<'_>) -> bool {
    local_eligible && !requires_unsupported_capability(requirements)
}

fn candidate_eligible(
    candidate: &WorkerCandidate<'_>,
    requirements: &SchedulingRequirements<'_>,
    now_unix_ms: u64,
) -> bool {
    // Worker probes are observations only. GPU and display task execution stay
    // unavailable until the runtime and device-grant policies are defined.
    if requires_unsupported_capability(requirements) {
        return false;
    }
    let worker = candidate.registration;
    let Some(probe) = worker.last_probe.as_ref() else {
        return false;
    };
    let maximum_execution_count = candidate
        .capabilities
        .maximum_execution_count
        .map_or(worker.max_jobs, |maximum| maximum.min(worker.max_jobs));
    if !worker.enabled
        || !requirements.dependencies_prepared
        || candidate.capabilities.docker_rootless != Some(true)
        || !worker
            .supported_architectures
            .iter()
            .any(|arch| arch == requirements.architecture)
        || !worker
            .trusted_image_digests
            .iter()
            .any(|digest| digest == requirements.image_digest)
        || probe.architecture.as_deref() != Some(requirements.architecture)
        || probe.policy_revision != requirements.policy_revision
        || candidate.capabilities.policy_hash != requirements.policy_revision
        || !candidate
            .capabilities
            .trusted_image_digests
            .iter()
            .any(|digest| digest == requirements.image_digest)
        || probe.observed_at_unix_ms > now_unix_ms
        || now_unix_ms.saturating_sub(probe.observed_at_unix_ms) > MAX_HEALTH_AGE_MS
        || probe.running_jobs >= maximum_execution_count
        || candidate.capabilities.current_execution_count >= maximum_execution_count
        || candidate
            .capabilities
            .available_cpu_millis
            .is_none_or(|available| available < requirements.resource_budget.cpu_millis)
        || candidate
            .capabilities
            .available_memory_bytes
            .is_none_or(|available| available < requirements.resource_budget.memory_bytes)
        || candidate
            .capabilities
            .disk_available_bytes
            .is_none_or(|available| available < requirements.resource_budget.temporary_disk_bytes)
        || unusable_health(&probe.health)
    {
        return false;
    }
    let request = requirements.resource_budget;
    let ceiling = &candidate.capabilities.resource_ceiling;
    request_fits(request, ceiling)
        && worker
            .resource_limit
            .as_ref()
            .is_none_or(|limit| request_fits(request, limit))
}

fn requires_unsupported_capability(requirements: &SchedulingRequirements<'_>) -> bool {
    requirements.requires_gpu || requirements.requires_display
}

fn request_fits(request: &ResourceBudgetV1, ceiling: &ResourceBudgetV1) -> bool {
    request.cpu_millis <= ceiling.cpu_millis
        && request.memory_bytes <= ceiling.memory_bytes
        && request.process_count <= ceiling.process_count
        && request.temporary_disk_bytes <= ceiling.temporary_disk_bytes
        && request.output_bytes <= ceiling.output_bytes
}

fn unusable_health(health: &RemoteWorkerHealth) -> bool {
    matches!(
        health,
        RemoteWorkerHealth::Busy
            | RemoteWorkerHealth::Disconnected
            | RemoteWorkerHealth::Untrusted
            | RemoteWorkerHealth::Incompatible
            | RemoteWorkerHealth::PolicyBlocked
    )
}

#[cfg(test)]
#[path = "scheduler/tests.rs"]
mod tests;
