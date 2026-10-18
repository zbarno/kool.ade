use crate::{
    execution::{
        backend::BackendCapabilitiesV1,
        contract::{ResourceBudgetV1, TargetLocation, TargetPreference},
        docker::{
            environment::DockerEnvironmentStatus,
            images::{ImageLock, RuntimeProfile},
        },
        remote::{
            client::RemoteWorkerClient,
            scheduler::{self, SchedulingRequirements, WorkerCandidate},
        },
    },
    harness::PlanningRequest,
    persistence::{
        harness_settings,
        remote_workers::{self, RemoteWorkerRegistration},
    },
};

use super::{ExecutionSession, can_fallback_for_preference, local_session};

struct ObservedWorker {
    registration: RemoteWorkerRegistration,
    capabilities: BackendCapabilitiesV1,
}

pub(super) fn auto_session(
    request: &PlanningRequest,
    workspace: crate::execution::contract::WorkspaceRef,
    profile: RuntimeProfile,
    resources: ResourceBudgetV1,
) -> anyhow::Result<ExecutionSession> {
    let local_eligible = DockerEnvironmentStatus::detect().ready();
    if request.authorization_verifier.is_none() {
        return auto_local_fallback(
            request,
            workspace,
            profile,
            resources,
            local_eligible,
            "Automatic remote routing requires an active task claim",
        );
    }
    let registrations = remote_workers::list().unwrap_or_default();
    let mut observed = Vec::new();
    for registration in registrations.into_iter().filter(|worker| worker.enabled) {
        let Ok(config) = super::remote::worker_config(&registration) else {
            continue;
        };
        let Ok(client) = RemoteWorkerClient::connect_for_probe(&config) else {
            continue;
        };
        let Some(capabilities) = client.probe().capabilities.clone() else {
            continue;
        };
        let registration = remote_workers::get(&registration.worker_id)
            .ok()
            .flatten()
            .unwrap_or(registration);
        observed.push(ObservedWorker {
            registration,
            capabilities,
        });
    }
    let lock = ImageLock::bundled().map_err(|failure| anyhow::anyhow!(failure.message))?;
    let worker_ids = observed
        .iter()
        .map(|worker| worker.registration.worker_id.clone())
        .collect::<Vec<_>>();
    let worker_order = remote_workers::order_auto_workers(&worker_ids).unwrap_or_else(|_| {
        let mut ids = worker_ids;
        ids.sort();
        ids.dedup();
        ids
    });
    let mut remote_choices = Vec::<(u32, usize, String)>::new();
    let architectures = observed
        .iter()
        .filter_map(|worker| {
            worker
                .registration
                .last_probe
                .as_ref()
                .and_then(|probe| probe.architecture.clone())
        })
        .collect::<std::collections::BTreeSet<_>>();
    let now = harness_settings::current_unix_ms();
    let policy_revision = crate::execution::policy::policy_hash();
    for architecture in architectures {
        let Ok(image) = lock.select(profile, &architecture) else {
            continue;
        };
        let candidates = observed
            .iter()
            .map(|worker| WorkerCandidate {
                registration: &worker.registration,
                capabilities: &worker.capabilities,
            })
            .collect::<Vec<_>>();
        let mut excluded = Vec::<String>::new();
        loop {
            let eligible_ids = worker_order
                .iter()
                .filter(|worker_id| {
                    !excluded.contains(worker_id)
                        && candidates.iter().any(|candidate| {
                            candidate.registration.worker_id.as_str() == worker_id.as_str()
                        })
                })
                .cloned()
                .collect::<Vec<_>>();
            let requirements = SchedulingRequirements {
                architecture: &architecture,
                image_digest: &image.digest,
                policy_revision: &policy_revision,
                resource_budget: &resources,
                remote_approved: true,
                dependencies_prepared: true,
                eligible_worker_ids: &eligible_ids,
                requires_gpu: false,
                requires_display: false,
            };
            let Ok(target) = scheduler::select_target(
                &TargetPreference::Auto,
                false,
                &candidates,
                &requirements,
                now,
            ) else {
                break;
            };
            let TargetLocation::Remote { worker_id } = target.location else {
                break;
            };
            excluded.push(worker_id.clone());
            let load = observed
                .iter()
                .find(|worker| worker.registration.worker_id == worker_id)
                .and_then(|worker| worker.registration.last_probe.as_ref())
                .map_or(u32::MAX, |probe| probe.running_jobs);
            let order = worker_order
                .iter()
                .position(|id| id == &worker_id)
                .unwrap_or(usize::MAX);
            if !remote_choices.iter().any(|(_, _, id)| id == &worker_id) {
                remote_choices.push((load, order, worker_id));
            }
        }
    }
    remote_choices.sort();
    let mut remaining = remote_choices
        .iter()
        .filter_map(|(load, _, worker_id)| {
            let worker = observed
                .iter()
                .find(|worker| worker.registration.worker_id == *worker_id)?;
            worker.registration.last_probe.as_ref()?;
            let max_jobs = worker
                .capabilities
                .maximum_execution_count
                .map_or(worker.registration.max_jobs, |maximum| {
                    maximum.min(worker.registration.max_jobs)
                });
            Some(remote_workers::AutoWorkerCapacityCandidate {
                worker_id: worker_id.clone(),
                host_key_fingerprint: worker.registration.host_key_fingerprint.clone(),
                max_jobs,
                observed_running_jobs: *load,
            })
        })
        .collect::<Vec<_>>();
    while !remaining.is_empty() {
        let Ok(Some(capacity)) = remote_workers::reserve_auto_capacity(&remaining) else {
            break;
        };
        let worker_id = capacity.worker_id().to_owned();
        remaining.retain(|candidate| candidate.worker_id != worker_id);
        match super::remote::remote_session_with_capacity(
            request,
            workspace.clone(),
            profile,
            resources.clone(),
            &worker_id,
            Some(capacity),
        ) {
            Ok(session) => return Ok(session),
            Err(_) => continue,
        }
    }
    auto_local_fallback(
        request,
        workspace,
        profile,
        resources,
        local_eligible,
        "No verified remote worker is eligible",
    )
}

fn auto_local_fallback(
    request: &PlanningRequest,
    workspace: crate::execution::contract::WorkspaceRef,
    profile: RuntimeProfile,
    resources: ResourceBudgetV1,
    local_eligible: bool,
    reason: &str,
) -> anyhow::Result<ExecutionSession> {
    anyhow::ensure!(
        can_fallback_for_preference(&TargetPreference::Auto, local_eligible),
        "{reason}; local Docker is unavailable"
    );
    local_session(request, workspace, profile, resources)
}
