use super::*;
use crate::execution::contract::ResourceBudgetV1;
const IMAGE_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn budget() -> ResourceBudgetV1 {
    ResourceBudgetV1 {
        cpu_millis: 2_000,
        memory_bytes: 512 * 1024 * 1024,
        process_count: 64,
        temporary_disk_bytes: 64 * 1024 * 1024,
        output_bytes: 1024 * 1024,
    }
}

fn worker(id: &str, running_jobs: u32, observed_at_unix_ms: u64) -> RemoteWorkerRegistration {
    RemoteWorkerRegistration {
        schema_version: 1,
        worker_id: id.into(),
        ssh_host_alias: format!("{id}.example"),
        ssh_principal: "worker".into(),
        host_key_fingerprint: format!("SHA256:{}", "A".repeat(43)),
        enrolled_at_unix_ms: 1,
        enabled: true,
        max_jobs: 3,
        supported_architectures: vec!["amd64".into()],
        trusted_image_digests: vec![IMAGE_DIGEST.into()],
        resource_limit: None,
        last_probe: Some(crate::persistence::remote_workers::RemoteWorkerProbe {
            observed_at_unix_ms,
            health: RemoteWorkerHealth::Healthy,
            architecture: Some("amd64".into()),
            running_jobs,
            policy_revision: "sha256:policy".into(),
            docker_version: Some("test".into()),
            trusted_image_digests: vec![IMAGE_DIGEST.into()],
            resource_ceiling: Some(capabilities(running_jobs).resource_ceiling),
            disk_available_bytes: Some(1024 * 1024 * 1024),
        }),
        policy_revision: "sha256:policy".into(),
    }
}

fn capabilities(running_jobs: u32) -> BackendCapabilitiesV1 {
    BackendCapabilitiesV1 {
        app_version: env!("CARGO_PKG_VERSION").into(),
        docker_version: Some("test".into()),
        docker_api_version: None,
        docker_rootless: Some(true),
        docker_desktop: Some(false),
        linux_containers: Some(true),
        seccomp_enabled: Some(true),
        memory_limit: Some(true),
        cpu_quota: Some(true),
        pids_limit: Some(true),
        disk_available_bytes: Some(1024 * 1024 * 1024),
        current_execution_count: running_jobs,
        maximum_execution_count: None,
        resource_ceiling: ResourceBudgetV1 {
            cpu_millis: 4_000,
            memory_bytes: 1024 * 1024 * 1024,
            process_count: 128,
            temporary_disk_bytes: 512 * 1024 * 1024,
            output_bytes: 16 * 1024 * 1024,
        },
        trusted_image_digests: vec![IMAGE_DIGEST.into()],
        policy_hash: "sha256:policy".into(),
    }
}

fn requirements<'a>(
    budget: &'a ResourceBudgetV1,
    eligible: &'a [String],
) -> SchedulingRequirements<'a> {
    SchedulingRequirements {
        architecture: "amd64",
        image_digest: IMAGE_DIGEST,
        policy_revision: "sha256:policy",
        resource_budget: budget,
        remote_approved: true,
        dependencies_prepared: true,
        eligible_worker_ids: eligible,
        requires_gpu: false,
        requires_display: false,
    }
}

#[test]
fn auto_selects_deterministically_by_load_and_excludes_stale_probes() {
    let now = 1_900_000_000_000;
    let loaded = worker("worker-a", 2, now - 1_000);
    let idle = worker("worker-b", 0, now - 1_000);
    let stale = worker("worker-c", 0, now - MAX_HEALTH_AGE_MS - 1);
    let loaded_caps = capabilities(2);
    let idle_caps = capabilities(0);
    let stale_caps = capabilities(0);
    let candidates = [
        WorkerCandidate {
            registration: &loaded,
            capabilities: &loaded_caps,
        },
        WorkerCandidate {
            registration: &idle,
            capabilities: &idle_caps,
        },
        WorkerCandidate {
            registration: &stale,
            capabilities: &stale_caps,
        },
    ];
    let budget = budget();
    let requirements = requirements(&budget, &[]);
    let selected = select_target(
        &TargetPreference::Auto,
        false,
        &candidates,
        &requirements,
        now,
    )
    .unwrap();
    assert_eq!(selected, ExecutionTarget::remote("worker-b"));
}

#[test]
fn auto_never_sends_to_remote_without_task_approval_and_honors_user_constraint() {
    let now = 1_900_000_000_000;
    let worker = worker("worker-a", 0, now);
    let caps = capabilities(0);
    let candidates = [WorkerCandidate {
        registration: &worker,
        capabilities: &caps,
    }];
    let budget = budget();
    let eligible = vec!["different-worker".into()];
    let mut requirements = requirements(&budget, &[]);
    requirements.remote_approved = false;
    assert_eq!(
        select_target(
            &TargetPreference::Auto,
            false,
            &candidates,
            &requirements,
            now
        ),
        Err(SelectionFailure::NoRemoteApproval)
    );
    requirements.remote_approved = true;
    requirements.eligible_worker_ids = &eligible;
    assert_eq!(
        select_target(
            &TargetPreference::Auto,
            false,
            &candidates,
            &requirements,
            now
        ),
        Err(SelectionFailure::NoCompatibleTarget)
    );
    assert_eq!(
        select_target(
            &TargetPreference::Worker("worker-a".into()),
            false,
            &candidates,
            &requirements,
            now
        ),
        Err(SelectionFailure::WorkerUnavailable)
    );
}

#[test]
fn auto_falls_back_locally_when_remote_requirements_cannot_be_met() {
    let now = 1_900_000_000_000;
    let worker = worker("worker-a", 0, now);
    let caps = capabilities(0);
    let candidates = [WorkerCandidate {
        registration: &worker,
        capabilities: &caps,
    }];
    let budget = budget();
    let mut requirements = requirements(&budget, &[]);
    requirements.requires_gpu = true;
    assert_eq!(
        select_target(
            &TargetPreference::Auto,
            true,
            &candidates,
            &requirements,
            now
        )
        .unwrap(),
        ExecutionTarget::local()
    );
}

#[test]
fn concurrent_two_worker_selection_respects_determinism_and_worker_capacity_claims() {
    use std::{
        sync::{Arc, Barrier},
        thread,
    };

    const CONTENDERS: usize = 9;
    let now = 1_900_000_000_000;
    let gate = Arc::new(Barrier::new(CONTENDERS + 1));
    let root = tempfile::tempdir().unwrap();
    let handles = (0..CONTENDERS)
        .map(|_| {
            let gate = gate.clone();
            let root = root.path().to_path_buf();
            thread::spawn(move || {
                let busy = worker("worker-a", 2, now);
                let idle = worker("worker-b", 0, now);
                let busy_caps = capabilities(2);
                let idle_caps = capabilities(0);
                let candidates = [
                    WorkerCandidate {
                        registration: &busy,
                        capabilities: &busy_caps,
                    },
                    WorkerCandidate {
                        registration: &idle,
                        capabilities: &idle_caps,
                    },
                ];
                let budget = budget();
                let requirements = requirements(&budget, &[]);
                let selected = select_target(
                    &TargetPreference::Auto,
                    false,
                    &candidates,
                    &requirements,
                    now,
                )
                .unwrap();
                assert_eq!(selected, ExecutionTarget::remote("worker-b"));

                let claim = super::super::worker_capacity::WorkerCapacityPool::new(
                    &root,
                    "00000000-0000-4000-8000-000000000001".into(),
                )
                .unwrap()
                .acquire();
                gate.wait();
                claim.is_ok()
            })
        })
        .collect::<Vec<_>>();
    gate.wait();
    let admitted = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .filter(|was_admitted| *was_admitted)
        .count();
    assert_eq!(
        admitted,
        super::super::worker_capacity::MAX_REMOTE_EXECUTIONS as usize
    );
}
