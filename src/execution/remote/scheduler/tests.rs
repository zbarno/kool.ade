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
            available_cpu_millis: Some(4_000),
            available_memory_bytes: Some(1024 * 1024 * 1024),
            cpu_pressure_some_avg10_basis_points: Some(running_jobs * 100),
            load_millis: Some(running_jobs * 500),
            gpu_available: Some(false),
            display_available: Some(false),
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
        available_cpu_millis: Some(4_000),
        available_memory_bytes: Some(1024 * 1024 * 1024),
        cpu_pressure_some_avg10_basis_points: Some(running_jobs * 100),
        load_millis: Some(running_jobs * 500),
        gpu_available: false,
        display_available: false,
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
fn auto_never_selects_a_worker_reported_busy_even_if_a_probe_snapshot_has_room() {
    let now = 1_900_000_000_000;
    let mut busy = worker("worker-busy", 0, now);
    busy.last_probe.as_mut().unwrap().health = RemoteWorkerHealth::Busy;
    let caps = capabilities(0);
    let candidates = [WorkerCandidate {
        registration: &busy,
        capabilities: &caps,
    }];
    let budget = budget();
    let requirements = requirements(&budget, &[]);

    assert_eq!(
        select_target(
            &TargetPreference::Auto,
            false,
            &candidates,
            &requirements,
            now,
        ),
        Err(SelectionFailure::NoCompatibleTarget)
    );
}

#[test]
fn auto_uses_reserved_cpu_capacity_without_treating_load_average_as_millicores() {
    let now = 1_900_000_000_000;
    let registration = worker("worker-loaded", 0, now);
    let mut loaded_capabilities = capabilities(0);
    loaded_capabilities.load_millis = Some(2_001);
    let candidates = [WorkerCandidate {
        registration: &registration,
        capabilities: &loaded_capabilities,
    }];
    let budget = budget();
    let selected = select_target(
        &TargetPreference::Auto,
        false,
        &candidates,
        &requirements(&budget, &[]),
        now,
    );
    assert_eq!(selected, Ok(ExecutionTarget::remote("worker-loaded")));
    let mut eligible_capabilities = capabilities(0);
    eligible_capabilities.load_millis = Some(1_999);
    let eligible = [WorkerCandidate {
        registration: &registration,
        capabilities: &eligible_capabilities,
    }];
    assert_eq!(
        select_target(
            &TargetPreference::Auto,
            false,
            &eligible,
            &requirements(&budget, &[]),
            now,
        ),
        Ok(ExecutionTarget::remote("worker-loaded"))
    );
}

#[path = "tests/capacity.rs"]
mod capacity;

#[test]
fn auto_rejects_workers_without_verified_rootless_docker() {
    let now = 1_900_000_000_000;
    let worker = worker("worker-rootful", 0, now);
    let mut caps = capabilities(0);
    caps.docker_rootless = Some(false);
    let candidates = [WorkerCandidate {
        registration: &worker,
        capabilities: &caps,
    }];
    let budget = budget();
    let requirements = requirements(&budget, &[]);

    assert_eq!(
        select_target(
            &TargetPreference::Auto,
            false,
            &candidates,
            &requirements,
            now,
        ),
        Err(SelectionFailure::NoCompatibleTarget)
    );

    caps.docker_rootless = None;
    let candidates = [WorkerCandidate {
        registration: &worker,
        capabilities: &caps,
    }];
    assert_eq!(
        select_target(
            &TargetPreference::Auto,
            false,
            &candidates,
            &requirements,
            now,
        ),
        Err(SelectionFailure::NoCompatibleTarget)
    );
}

#[test]
fn auto_rotates_equal_load_workers_in_the_durable_cursor_order() {
    let now = 1_900_000_000_000;
    let worker_a = worker("worker-a", 0, now);
    let worker_b = worker("worker-b", 0, now);
    let worker_c = worker("worker-c", 0, now);
    let caps_a = capabilities(0);
    let caps_b = capabilities(0);
    let caps_c = capabilities(0);
    let candidates = [
        WorkerCandidate {
            registration: &worker_a,
            capabilities: &caps_a,
        },
        WorkerCandidate {
            registration: &worker_b,
            capabilities: &caps_b,
        },
        WorkerCandidate {
            registration: &worker_c,
            capabilities: &caps_c,
        },
    ];
    let budget = budget();
    let cursor_order = vec!["worker-c".into(), "worker-a".into(), "worker-b".into()];
    let requirements = requirements(&budget, &cursor_order);

    assert_eq!(
        select_target(
            &TargetPreference::Auto,
            false,
            &candidates,
            &requirements,
            now,
        )
        .unwrap(),
        ExecutionTarget::remote("worker-c")
    );
}

#[path = "tests/constraints.rs"]
mod constraints;
