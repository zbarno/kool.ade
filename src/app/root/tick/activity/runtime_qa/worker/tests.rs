use super::*;

use super::super::test_support::dispatch_fixture;
#[path = "tests/restart.rs"]
mod restart;

#[test]
fn held_qa_run_lock_is_reported_as_active_worker_contention() {
    let root = tempfile::tempdir().unwrap();
    let lock_path = root.path().join("run").join("worker.lock");
    let owner = acquire_worker_lock(&lock_path).unwrap();

    assert_eq!(
        acquire_worker_lock(&lock_path).unwrap_err(),
        RunLockError::ActiveWorker
    );
    drop(owner);
    assert!(acquire_worker_lock(&lock_path).is_ok());
}

#[test]
// Production boundary: the per-run lock is acquired before the file-backed
// lifecycle compare-and-swap, so a request replaced after approval cannot
// launch from a stale revision. Two store handles model separate app instances;
// no Docker, network, or credentials are needed, with low setup cost.
fn stale_request_replacement_wins_before_dispatch_claim_and_no_worker_lock_leaks() {
    let root = tempfile::tempdir().unwrap();
    let project_id = uuid::Uuid::new_v4();
    let planning =
        crate::artifacts::planning_store::PlanningStore::legacy_embedded(project_id, root.path());
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, revision) = dispatch_fixture::queued_task(&lifecycle);
    let other_planning =
        crate::artifacts::planning_store::PlanningStore::legacy_embedded(project_id, root.path());
    let other_lifecycle = crate::core::lifecycle::LifecycleStore::new(&other_planning);
    let mut replacement = request.clone();
    replacement.qa_run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let replace_event = uuid::Uuid::new_v4().hyphenated().to_string();
    other_lifecycle
        .queue_runtime_qa(&task_uid, revision, &replace_event, replacement.clone())
        .unwrap();

    let lock_path = root
        .path()
        .join("runtime-qa")
        .join(&request.qa_run_id)
        .join("worker.lock");
    let result = acquire_lock_and_claim(&lock_path, &lifecycle, &task_uid, revision, &request);

    assert!(
        matches!(result, Err(StartError::Failed(reason)) if reason.contains("dispatch claim failed"))
    );
    assert!(acquire_worker_lock(&lock_path).is_ok());
    let task = lifecycle.load_task(&task_uid).unwrap();
    let execution = task.qa_execution.unwrap();
    assert_eq!(
        execution.status,
        crate::core::lifecycle::QaExecutionStatusV1::Pending
    );
    assert_eq!(execution.request, Some(replacement));
}

#[test]
// Production boundary: the claim is durably tied to the exact pre-dispatch
// approval revision, while the OS lock prevents a second app instance from
// launching the same run. File-backed stores only; no Docker or network.
fn stale_running_request_without_report_fails_closed_after_lock_recovery() {
    let root = tempfile::tempdir().unwrap();
    let project_id = uuid::Uuid::new_v4();
    let planning =
        crate::artifacts::planning_store::PlanningStore::legacy_embedded(project_id, root.path());
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, revision) = dispatch_fixture::queued_task(&lifecycle);
    let queued = lifecycle.load_task(&task_uid).unwrap();
    let request_revision = queued.qa_execution.as_ref().unwrap().request_task_revision;
    assert_eq!(request_revision, Some(revision - 1));
    assert_eq!(
        super::super::request::request_task_revision(&queued),
        request_revision.unwrap()
    );
    let lock_path = root
        .path()
        .join("runtime-qa")
        .join(&request.qa_run_id)
        .join("worker.lock");

    let (lock, claimed) =
        acquire_lock_and_claim(&lock_path, &lifecycle, &task_uid, revision, &request).unwrap();
    assert_eq!(
        claimed
            .qa_execution
            .as_ref()
            .unwrap()
            .dispatch_task_revision,
        Some(revision)
    );
    assert_eq!(
        claimed.qa_execution.as_ref().unwrap().request_task_revision,
        request_revision
    );
    assert_eq!(
        claimed.qa_execution.as_ref().unwrap().status,
        crate::core::lifecycle::QaExecutionStatusV1::Running
    );
    let mut replacement = request.clone();
    replacement.qa_run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let replace_event = uuid::Uuid::new_v4().hyphenated().to_string();
    assert!(
        lifecycle
            .queue_runtime_qa(&task_uid, claimed.revision, &replace_event, replacement)
            .is_err()
    );
    assert!(matches!(
        acquire_lock_and_claim(
            &lock_path,
            &lifecycle,
            &task_uid,
            claimed.revision,
            &request
        ),
        Err(StartError::ActiveWorker)
    ));
    drop(lock);

    let result = acquire_lock_and_claim(
        &lock_path,
        &lifecycle,
        &task_uid,
        claimed.revision,
        &request,
    );
    assert!(
        matches!(result, Err(StartError::Failed(reason)) if reason.contains("fresh reviewed request"))
    );
    assert!(acquire_worker_lock(&lock_path).is_ok());
    let stopped = lifecycle.load_task(&task_uid).unwrap();
    assert_eq!(stopped.revision, claimed.revision);
    let execution = stopped.qa_execution.as_ref().unwrap();
    assert_eq!(
        execution.status,
        crate::core::lifecycle::QaExecutionStatusV1::Running
    );
    assert_eq!(
        super::super::request::request_task_revision(&stopped),
        request_revision.unwrap()
    );
    assert_eq!(execution.dispatch_task_revision, Some(revision));
}
