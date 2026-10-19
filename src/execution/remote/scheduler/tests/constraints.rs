use super::*;

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
fn auto_does_not_select_an_incompatible_local_target_for_gpu_requirements() {
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
        ),
        Err(SelectionFailure::NoCompatibleTarget)
    );
}

#[test]
fn local_target_rejects_unavailable_gpu_or_display_requirements() {
    let now = 1_900_000_000_000;
    let budget = budget();
    let candidates = [];
    let mut requirements = requirements(&budget, &[]);
    requirements.requires_gpu = true;
    assert_eq!(
        select_target(
            &TargetPreference::Local,
            true,
            &candidates,
            &requirements,
            now
        ),
        Err(SelectionFailure::NoCompatibleTarget)
    );
    requirements.requires_gpu = false;
    requirements.requires_display = true;
    assert_eq!(
        select_target(
            &TargetPreference::Local,
            true,
            &candidates,
            &requirements,
            now
        ),
        Err(SelectionFailure::NoCompatibleTarget)
    );
}

#[test]
fn auto_selects_remote_worker_when_required_gpu_is_reported() {
    let now = 1_900_000_000_000;
    let worker = worker("worker-a", 0, now);
    let mut caps = capabilities(0);
    caps.gpu_available = true;
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
        ExecutionTarget::remote("worker-a")
    );
}

#[test]
fn missing_worker_capability_fields_default_to_unsupported() {
    let mut value = serde_json::to_value(capabilities(0)).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("gpu_available");
    object.remove("display_available");
    object.remove("available_cpu_millis");
    object.remove("available_memory_bytes");
    object.remove("cpu_pressure_some_avg10_basis_points");
    object.remove("load_millis");
    let restored: crate::execution::backend::BackendCapabilitiesV1 =
        serde_json::from_value(value).unwrap();
    assert!(!restored.gpu_available);
    assert!(!restored.display_available);
    assert_eq!(restored.available_cpu_millis, None);
    assert_eq!(restored.available_memory_bytes, None);
    assert_eq!(restored.cpu_pressure_some_avg10_basis_points, None);
    assert_eq!(restored.load_millis, None);
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

                let claim = super::super::super::worker_capacity::WorkerCapacityPool::new(
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
        super::super::super::worker_capacity::MAX_REMOTE_EXECUTIONS as usize
    );
}
