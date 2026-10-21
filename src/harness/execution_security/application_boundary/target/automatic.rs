use crate::{
    execution::{
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
    persistence::{harness_settings, remote_workers},
};

use super::ExecutionSession;
mod diagnostics;
mod eligibility;
mod fallback;
mod observed;
mod retry;
use diagnostics::retry_test_diagnostic;
#[cfg(test)]
use eligibility::worker_target_allowed;
use eligibility::{
    local_target_eligible, ordered_remaining_workers, select_from_remaining, worker_retry_eligible,
};
use fallback::auto_local_fallback;
use observed::ObservedWorker;

#[cfg(test)]
#[path = "automatic/tests.rs"]
mod tests;

pub(super) fn auto_session_excluding(
    request: &PlanningRequest,
    workspace: crate::execution::contract::WorkspaceRef,
    profile: RuntimeProfile,
    resources: ResourceBudgetV1,
    requirements: crate::persistence::execution_environment_settings::TaskCapabilityRequirements,
    allowed_targets: Vec<TargetPreference>,
    excluded_workers: Vec<String>,
) -> anyhow::Result<ExecutionSession> {
    retry_test_diagnostic(|| "automatic routing started".to_owned());
    let local_eligible =
        local_target_eligible(DockerEnvironmentStatus::detect().ready(), &allowed_targets);
    if request.authorization_verifier.is_none() {
        return auto_local_fallback(
            request,
            workspace,
            profile,
            resources,
            local_eligible,
            requirements,
            "Automatic remote routing requires an active task claim",
        );
    }
    let registrations = remote_workers::list().unwrap_or_default();
    retry_test_diagnostic(|| {
        format!(
            "automatic routing found {} enrolled workers",
            registrations.len()
        )
    });
    let mut observed = Vec::new();
    for registration in registrations.into_iter().filter(|worker| {
        worker.enabled
            && worker_retry_eligible(&worker.worker_id, &allowed_targets, &excluded_workers)
    }) {
        let Ok(config) = super::remote::worker_config(&registration) else {
            continue;
        };
        retry_test_diagnostic(|| "automatic routing probing an enrolled worker".to_owned());
        let Ok(client) = RemoteWorkerClient::connect_for_probe(&config) else {
            continue;
        };
        let Some(capabilities) = client.probe().capabilities.clone() else {
            continue;
        };
        retry_test_diagnostic(|| "automatic routing received worker capabilities".to_owned());
        let registration = remote_workers::get(&registration.worker_id)
            .ok()
            .flatten()
            .unwrap_or(registration);
        observed.push(ObservedWorker {
            registration,
            capabilities,
        });
    }
    retry_test_diagnostic(|| "automatic routing collected worker capabilities".to_owned());
    retry_test_diagnostic(|| "automatic routing loading the bundled image lock".to_owned());
    let lock = ImageLock::bundled().map_err(|failure| anyhow::anyhow!(failure.message))?;
    retry_test_diagnostic(|| "automatic routing loaded the bundled image lock".to_owned());
    let worker_ids = observed
        .iter()
        .map(|worker| worker.registration.worker_id.clone())
        .collect::<Vec<_>>();
    let observed_worker_ids = worker_ids.clone();
    retry_test_diagnostic(|| "automatic routing reading the worker order cursor".to_owned());
    let worker_order = remote_workers::order_auto_workers(&worker_ids).unwrap_or_else(|_| {
        let mut ids = worker_ids;
        ids.sort();
        ids.dedup();
        ids
    });
    retry_test_diagnostic(|| "automatic routing resolved the worker order".to_owned());
    let mut remote_choices = Vec::<(
        std::cmp::Reverse<u32>,
        std::cmp::Reverse<u64>,
        u32,
        u32,
        u32,
        usize,
        String,
    )>::new();
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
        retry_test_diagnostic(|| {
            format!("automatic routing selecting a target for {architecture}")
        });
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
            let eligible_ids =
                ordered_remaining_workers(&worker_order, &observed_worker_ids, &excluded);
            let Some(target) = select_from_remaining(&eligible_ids, |eligible_ids| {
                let requirements = SchedulingRequirements {
                    architecture: &architecture,
                    image_digest: &image.digest,
                    policy_revision: &policy_revision,
                    resource_budget: &resources,
                    remote_approved: true,
                    dependencies_prepared: true,
                    eligible_worker_ids: eligible_ids,
                    requires_gpu: requirements.requires_gpu,
                    requires_display: requirements.requires_display,
                };
                retry_test_diagnostic(|| {
                    format!("automatic routing evaluating candidates for {architecture}")
                });
                scheduler::select_target(
                    &TargetPreference::Auto,
                    false,
                    &candidates,
                    &requirements,
                    now,
                )
                .ok()
            }) else {
                break;
            };
            retry_test_diagnostic(|| {
                format!("automatic routing selected a candidate for {architecture}")
            });
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
            if !remote_choices
                .iter()
                .any(|(_, _, _, _, _, _, id)| id == &worker_id)
            {
                let worker = observed
                    .iter()
                    .find(|worker| worker.registration.worker_id == worker_id);
                let Some(capabilities) = worker.map(|worker| &worker.capabilities) else {
                    continue;
                };
                remote_choices.push((
                    std::cmp::Reverse(capabilities.available_cpu_millis.unwrap_or_default()),
                    std::cmp::Reverse(capabilities.available_memory_bytes.unwrap_or_default()),
                    capabilities
                        .cpu_pressure_some_avg10_basis_points
                        .unwrap_or(u32::MAX),
                    capabilities.load_millis.unwrap_or(u32::MAX),
                    load,
                    order,
                    worker_id,
                ));
            }
        }
    }
    remote_choices.sort();
    retry_test_diagnostic(|| {
        format!(
            "automatic routing selected {} capacity candidates",
            remote_choices.len()
        )
    });
    let mut remaining = remote_choices
        .iter()
        .filter_map(|(_, _, _, _, load, _, worker_id)| {
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
        retry_test_diagnostic(|| "automatic routing reserving worker capacity".to_owned());
        let Ok(Some(capacity)) = remote_workers::reserve_auto_capacity(&remaining) else {
            break;
        };
        let worker_id = capacity.worker_id().to_owned();
        remaining.retain(|candidate| candidate.worker_id != worker_id);
        retry_test_diagnostic(|| {
            "automatic routing opening the reserved worker session".to_owned()
        });
        match super::remote::remote_session_with_capacity(
            request,
            workspace.clone(),
            profile,
            resources.clone(),
            &worker_id,
            requirements,
            Some(capacity),
        ) {
            Ok(session) => {
                retry_test_diagnostic(|| "automatic routing opened the worker session".to_owned());
                retry::install_factory(
                    &session,
                    request,
                    &workspace,
                    profile,
                    &resources,
                    requirements,
                    &allowed_targets,
                    &excluded_workers,
                    worker_id,
                )?;
                return Ok(session);
            }
            Err(error) => {
                retry_test_diagnostic(|| {
                    format!("automatic routing rejected a worker session: {error:#}")
                });
                continue;
            }
        }
    }
    if !excluded_workers.is_empty() {
        anyhow::bail!("No alternate trusted remote worker is eligible after confirmed worker loss");
    }
    auto_local_fallback(
        request,
        workspace,
        profile,
        resources,
        local_eligible,
        requirements,
        "No verified remote worker is eligible",
    )
}
