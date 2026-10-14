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
    let mut remote_choices = Vec::<(u32, String)>::new();
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
            let eligible_ids = candidates
                .iter()
                .map(|candidate| candidate.registration.worker_id.clone())
                .filter(|worker_id| !excluded.contains(worker_id))
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
            if !remote_choices.iter().any(|(_, id)| id == &worker_id) {
                remote_choices.push((load, worker_id));
            }
        }
    }
    remote_choices.sort();
    for (_, worker_id) in remote_choices {
        match super::remote::remote_session(
            request,
            workspace.clone(),
            profile,
            resources.clone(),
            &worker_id,
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
