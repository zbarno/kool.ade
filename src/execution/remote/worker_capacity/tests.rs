use super::{MAX_REMOTE_EXECUTIONS, WorkerCapacityPool};
use std::{
    sync::{Arc, Barrier},
    thread,
};

#[test]
fn worker_slots_are_global_across_independent_controller_pools() {
    let root = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let root_path = root.path().to_path_buf();
    let barrier_a = barrier.clone();
    let a = thread::spawn(move || {
        let pool =
            WorkerCapacityPool::new(&root_path, "00000000-0000-4000-8000-000000000001".into())
                .unwrap();
        let mut leases = Vec::new();
        for _ in 0..MAX_REMOTE_EXECUTIONS - 1 {
            leases.push(pool.acquire().unwrap());
        }
        barrier_a.wait();
        let acquired = pool.acquire().ok();
        barrier_a.wait();
        let result = acquired.is_some();
        drop(leases);
        drop(acquired);
        result
    });
    let root_path = root.path().to_path_buf();
    let barrier_b = barrier.clone();
    let b = thread::spawn(move || {
        let pool =
            WorkerCapacityPool::new(&root_path, "00000000-0000-4000-8000-000000000001".into())
                .unwrap();
        barrier_b.wait();
        let acquired = pool.acquire().ok();
        barrier_b.wait();
        acquired.is_some()
    });
    let admitted = [a.join().unwrap(), b.join().unwrap()]
        .into_iter()
        .filter(|was_admitted| *was_admitted)
        .count();
    assert_eq!(admitted, 1);
}

#[test]
fn worker_slot_is_released_on_disconnect_and_isolated_by_worker_identity() {
    let root = tempfile::tempdir().unwrap();
    let pool = WorkerCapacityPool::new(root.path(), "00000000-0000-4000-8000-000000000001".into())
        .unwrap();
    let lease = pool.acquire().unwrap();
    drop(lease);
    assert!(pool.acquire().is_ok());
    assert!(
        WorkerCapacityPool::new(root.path(), "00000000-0000-4000-8000-000000000002".into())
            .unwrap()
            .acquire()
            .is_ok()
    );
}

#[test]
fn resource_budget_leases_are_shared_across_worker_sessions_and_released_on_drop() {
    let root = tempfile::tempdir().unwrap();
    let worker_id = "00000000-0000-4000-8000-000000000001";
    let session_a = WorkerCapacityPool::new(root.path(), worker_id.into()).unwrap();
    let session_b = WorkerCapacityPool::new(root.path(), worker_id.into()).unwrap();
    let lease = session_a
        .acquire_resources("exec-a", 3_000, 700, 4_000, 1_000)
        .unwrap();
    assert_eq!(session_b.reserved_resources().unwrap().cpu_millis, 3_000);
    assert_eq!(session_b.reserved_resources().unwrap().memory_bytes, 700);
    assert!(
        session_b
            .acquire_resources("exec-b", 2_000, 100, 4_000, 1_000)
            .is_err()
    );
    drop(lease);
    assert_eq!(session_b.reserved_resources().unwrap().cpu_millis, 0);
    assert_eq!(session_b.reserved_resources().unwrap().memory_bytes, 0);
    assert!(
        session_b
            .acquire_resources("exec-b", 4_000, 1_000, 4_000, 1_000)
            .is_ok()
    );
}

#[test]
fn concurrent_sessions_cannot_overbook_the_shared_cpu_budget() {
    let root = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let root_a = root.path().to_path_buf();
    let barrier_a = barrier.clone();
    let a = thread::spawn(move || {
        let pool = WorkerCapacityPool::new(&root_a, "00000000-0000-4000-8000-000000000001".into())
            .unwrap();
        barrier_a.wait();
        let lease = pool.acquire_resources("exec-a", 4_000, 0, 4_000, 1).ok();
        barrier_a.wait();
        let acquired = lease.is_some();
        drop(lease);
        acquired
    });
    let root_b = root.path().to_path_buf();
    let barrier_b = barrier.clone();
    let b = thread::spawn(move || {
        let pool = WorkerCapacityPool::new(&root_b, "00000000-0000-4000-8000-000000000001".into())
            .unwrap();
        barrier_b.wait();
        let lease = pool.acquire_resources("exec-b", 4_000, 0, 4_000, 1).ok();
        barrier_b.wait();
        let acquired = lease.is_some();
        drop(lease);
        acquired
    });
    assert_eq!(
        [a.join().unwrap(), b.join().unwrap()]
            .into_iter()
            .filter(|ok| *ok)
            .count(),
        1
    );
}

#[test]
fn concurrent_sessions_cannot_overbook_the_shared_memory_budget() {
    let root = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let worker_id = "00000000-0000-4000-8000-000000000001";
    let make_attempt = |execution_id: &'static str, barrier: Arc<Barrier>| {
        let root = root.path().to_path_buf();
        thread::spawn(move || {
            let pool = WorkerCapacityPool::new(&root, worker_id.into()).unwrap();
            barrier.wait();
            let lease = pool
                .acquire_resources(execution_id, 100, 800, 4_000, 1_000)
                .ok();
            barrier.wait();
            lease.is_some()
        })
    };
    let first = make_attempt("exec-a", barrier.clone());
    let second = make_attempt("exec-b", barrier);
    assert_eq!(
        [first.join().unwrap(), second.join().unwrap()]
            .into_iter()
            .filter(|acquired| *acquired)
            .count(),
        1
    );
}

#[test]
fn durable_execution_records_keep_resources_reserved_after_worker_process_exit() {
    use crate::execution::policy::ExecutionPolicyV1;
    use crate::{
        execution::contract::{
            ExecutionModeV1, ExecutionRequestV1, ExecutionTarget, ResourceBudgetV1, WorkspaceRef,
        },
        persistence::execution_records::{
            DockerResourceUsageV1, ExecutionCleanupStateV1, ExecutionPhaseTimingsV1,
            ExecutionRecordStateV1, ExecutionRecordV1,
        },
    };

    let root = tempfile::tempdir().unwrap();
    let worker_id = "00000000-0000-4000-8000-000000000101";
    let owner_id =
        crate::persistence::execution_records::owner_instance_id_at(root.path()).unwrap();
    let execution_id = "00000000-0000-4000-8000-000000000102";
    let project_id = "00000000-0000-4000-8000-000000000103";
    let task_id = "00000000-0000-4000-8000-000000000104";
    let budget = ResourceBudgetV1 {
        cpu_millis: 2_000,
        memory_bytes: 700 * 1024 * 1024,
        process_count: 10,
        temporary_disk_bytes: 64 * 1024 * 1024,
        output_bytes: 4_096,
    };
    let mode = ExecutionModeV1::Implementation;
    let request = ExecutionRequestV1 {
        schema_version: 1,
        execution_id: execution_id.into(),
        authorization_generation: "00000000-0000-4000-8000-000000000105".into(),
        project_id: project_id.into(),
        task_id: task_id.into(),
        target: ExecutionTarget::remote(worker_id),
        controller_target_id: Some(worker_id.into()),
        workspace: WorkspaceRef {
            project_id: project_id.into(),
            task_id: task_id.into(),
            source_sha: "a".repeat(40),
        },
        destination_sha: None,
        mode,
        image_digest: format!("sha256:{}", "b".repeat(64)),
        argv: vec!["git".into()],
        working_directory_relative: ".".into(),
        stdin: None,
        environment_allowlist: Default::default(),
        resource_budget: budget.clone(),
        deadline_unix_ms: 86_401_000,
        allowed_service_ports: Default::default(),
        broker_capability_ids: Default::default(),
        dependency_cache_refs: Vec::new(),
        cancellation_generation: 1,
        request_sequence: 1,
        policy: ExecutionPolicyV1::for_mode(mode, budget, Default::default()).unwrap(),
    };
    request.validate_at(1_000).unwrap();
    let mut record = ExecutionRecordV1 {
        schema_version: 1,
        execution_id: execution_id.into(),
        request: Some(request),
        project_id: project_id.into(),
        task_id: task_id.into(),
        target_id: worker_id.into(),
        controller_target_id: Some(worker_id.into()),
        owner_instance_id: owner_id,
        container_name: Some(format!("koolade-exec-{execution_id}")),
        workspace_lease_id: None,
        workspace_source_sha: "a".repeat(40),
        destination_sha: None,
        workspace_manifest: None,
        container_id: None,
        command_id: None,
        command_started: false,
        result: None,
        image_digest: format!("sha256:{}", "b".repeat(64)),
        policy_version: "docker-sandbox-v1".into(),
        lease_generation: 1,
        lease_expires_at_unix_ms: Some(10_000),
        attempt: 1,
        state: ExecutionRecordStateV1::Running,
        event_cursor: 0,
        output_bytes: 0,
        started_at_unix_ms: 1_000,
        updated_at_unix_ms: 1_000,
        ended_at_unix_ms: None,
        exit_code: None,
        artifacts: Vec::new(),
        docker_usage: DockerResourceUsageV1::default(),
        phase_timings: ExecutionPhaseTimingsV1::default(),
        cleanup: ExecutionCleanupStateV1::Pending,
    };
    crate::persistence::execution_records::save_at(root.path(), &record).unwrap();

    let restarted = WorkerCapacityPool::new(root.path(), worker_id.into()).unwrap();
    let reserved = restarted.reserved_resources().unwrap();
    assert_eq!(reserved.cpu_millis, 2_000);
    assert_eq!(reserved.memory_bytes, 700 * 1024 * 1024);
    assert!(
        restarted
            .acquire_resources(
                "00000000-0000-4000-8000-000000000106",
                1_000,
                400 * 1024 * 1024,
                4_000,
                1_000 * 1024 * 1024
            )
            .is_err()
    );
    assert!(
        restarted
            .acquire_resources(
                execution_id,
                1_000,
                400 * 1024 * 1024,
                4_000,
                1_000 * 1024 * 1024
            )
            .is_err()
    );

    let mut local_request = record.request.clone().unwrap();
    local_request.target = ExecutionTarget::local();
    local_request.controller_target_id = None;
    record.request = Some(local_request);
    record.target_id = "local".into();
    record.controller_target_id = None;
    crate::persistence::execution_records::save_at(root.path(), &record).unwrap();
    let local_pool = WorkerCapacityPool::new_local_unknown(root.path()).unwrap();
    assert!(local_pool.acquire_unknown_local().is_err());

    record.state = ExecutionRecordStateV1::Interrupted;
    record.updated_at_unix_ms = 2_000;
    record.ended_at_unix_ms = Some(2_000);
    record.cleanup = ExecutionCleanupStateV1::Complete;
    crate::persistence::execution_records::save_at(root.path(), &record).unwrap();
    assert_eq!(restarted.reserved_resources().unwrap().cpu_millis, 0);
    assert_eq!(restarted.reserved_resources().unwrap().memory_bytes, 0);
    assert!(local_pool.acquire_unknown_local().is_ok());
}
