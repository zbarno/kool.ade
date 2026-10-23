use super::*;
use crate::core::lifecycle::QaExecutionStatusV1;

#[path = "tests/completed_report.rs"]
mod completed_report;
use super::super::super::test_support::dispatch_fixture as fixture;

#[test]
// Production boundary: a failed worker start must write its blocker through
// LifecycleStore before the dispatcher can drop the launch attempt. Local
// file-backed store and synthetic records only; no Docker or network setup.
fn failed_worker_start_persists_block_and_returns_no_handle() {
    let root = tempfile::tempdir().unwrap();
    let planning = crate::artifacts::planning_store::PlanningStore::legacy_embedded(
        uuid::Uuid::new_v4(),
        root.path(),
    );
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, _revision) = fixture::queued_task(&lifecycle);
    let run_id = request.qa_run_id.clone();
    let qa_root = root.path().join("qa");
    let qa_store = crate::runtime_qa::RuntimeQaStore::new(&qa_root);
    let lock_path = qa_root.join(&run_id).join("worker.lock");
    let result = start_worker_or_block(
        &lifecycle,
        &qa_store,
        &task_uid,
        &request,
        &lock_path,
        || {
            Err(worker::StartError::Failed(
                "simulated worker thread spawn failure".into(),
            ))
        },
    );
    assert!(
        matches!(result, Err(DispatchStartError::Blocked { reason, persistence_error: None }) if reason == "simulated worker thread spawn failure")
    );

    let task = lifecycle.load_task(&task_uid).unwrap();
    let execution = task.qa_execution.unwrap();
    assert_eq!(execution.status, QaExecutionStatusV1::Blocked);
    assert_eq!(execution.request.unwrap().qa_run_id, run_id);
    assert_eq!(
        execution.blocked_reason.as_deref(),
        Some("simulated worker thread spawn failure")
    );
    let reopened = crate::artifacts::planning_store::PlanningStore::legacy_embedded(
        planning.project_id,
        root.path(),
    );
    let recovered = crate::core::lifecycle::LifecycleStore::new(&reopened)
        .load_task(&task_uid)
        .unwrap();
    assert_eq!(
        recovered.qa_execution.unwrap().status,
        QaExecutionStatusV1::Blocked
    );
}

#[test]
// Production boundary: a recovered Running lifecycle claim with no report
// must stop before replaying driver work and persist a fresh-review blocker.
// File-backed lifecycle/evidence stores and the OS run lock are exercised;
// no Docker or network is needed, with low setup cost.
fn stale_running_attempt_without_durable_report_is_blocked_for_fresh_review() {
    let root = tempfile::tempdir().unwrap();
    let planning = crate::artifacts::planning_store::PlanningStore::legacy_embedded(
        uuid::Uuid::new_v4(),
        root.path(),
    );
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, revision) = fixture::queued_task(&lifecycle);
    let running = lifecycle
        .claim_runtime_qa_worker(
            &task_uid,
            revision,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            &request,
        )
        .unwrap();
    let qa_root = root.path().join("qa");
    let qa_store = crate::runtime_qa::RuntimeQaStore::new(&qa_root);
    let lock_path = qa_root.join(&request.qa_run_id).join("worker.lock");
    let result = start_worker_or_block(
        &lifecycle,
        &qa_store,
        &task_uid,
        &request,
        &lock_path,
        || {
            Err(worker::acquire_lock_and_claim(
                &lock_path,
                &lifecycle,
                &task_uid,
                running.revision,
                &request,
            )
            .unwrap_err())
        },
    );

    assert!(
        matches!(result, Err(DispatchStartError::Blocked { reason, persistence_error: None }) if reason.contains("fresh reviewed request"))
    );
    let blocked = lifecycle.load_task(&task_uid).unwrap();
    let execution = blocked.qa_execution.unwrap();
    assert_eq!(execution.status, QaExecutionStatusV1::Blocked);
    assert!(
        execution
            .blocked_reason
            .unwrap()
            .contains("fresh reviewed request")
    );
}

#[test]
// Production boundary: a failed start must reacquire the per-run lock before
// writing a blocker, so another app's live run stays pending. The test uses a
// real file lock and file-backed lifecycle; no Docker or network, low setup.
fn active_worker_contention_keeps_qa_run_pending() {
    let root = tempfile::tempdir().unwrap();
    let planning = crate::artifacts::planning_store::PlanningStore::legacy_embedded(
        uuid::Uuid::new_v4(),
        root.path(),
    );
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, _revision) = fixture::queued_task(&lifecycle);
    let run_id = request.qa_run_id.clone();
    let qa_root = root.path().join("qa");
    let qa_store = crate::runtime_qa::RuntimeQaStore::new(&qa_root);
    let lock_path = qa_root.join(&run_id).join("worker.lock");
    let worker_lock = worker::acquire_run_lock(&lock_path).unwrap();
    let result = start_worker_or_block(
        &lifecycle,
        &qa_store,
        &task_uid,
        &request,
        &lock_path,
        || Err(worker::StartError::Failed("simulated restart race".into())),
    );

    assert!(matches!(result, Err(DispatchStartError::ActiveWorker)));
    let task = lifecycle.load_task(&task_uid).unwrap();
    let execution = task.qa_execution.unwrap();
    assert_eq!(execution.status, QaExecutionStatusV1::Pending);
    assert_eq!(execution.request.unwrap().qa_run_id, run_id);
    drop(worker_lock);
}

#[test]
// Production boundary: a valid report written while a restart attempt is
// failing must survive the handler's lock reacquisition instead of being
// replaced by a blocker. Uses a file-backed lifecycle, QA store, and lock;
// no Docker or network setup.
fn persisted_report_wins_over_dispatch_failure_after_lock_reacquisition() {
    let root = tempfile::tempdir().unwrap();
    let planning = crate::artifacts::planning_store::PlanningStore::legacy_embedded(
        uuid::Uuid::new_v4(),
        root.path(),
    );
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, revision) = fixture::queued_task(&lifecycle);
    lifecycle
        .claim_runtime_qa_worker(
            &task_uid,
            revision,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            &request,
        )
        .unwrap();
    let qa_root = root.path().join("qa");
    let qa_store = crate::runtime_qa::RuntimeQaStore::new(&qa_root);
    let lock_path = qa_root.join(&request.qa_run_id).join("worker.lock");
    let result = start_worker_or_block(
        &lifecycle,
        &qa_store,
        &task_uid,
        &request,
        &lock_path,
        || {
            completed_report::save_infrastructure_report(&qa_store, &task_uid, &request);
            Err(worker::StartError::Failed("simulated restart race".into()))
        },
    );

    assert!(matches!(result, Err(DispatchStartError::ReportPresent)));
    assert_eq!(
        lifecycle
            .load_task(&task_uid)
            .unwrap()
            .qa_execution
            .unwrap()
            .status,
        QaExecutionStatusV1::Running
    );
}

#[test]
fn approved_dispatch_requires_the_exact_pending_task_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let planning = crate::artifacts::planning_store::PlanningStore::legacy_embedded(
        uuid::Uuid::new_v4(),
        root.path(),
    );
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, revision) = fixture::queued_task(&lifecycle);
    let task = lifecycle.load_task(&task_uid).unwrap();

    assert!(matches_pending_launch(&task, revision, &request));

    let mut stale = task.clone();
    stale.revision += 1;
    assert!(!matches_pending_launch(&stale, revision, &request));

    let mut running = lifecycle.load_task(&task_uid).unwrap();
    running.revision += 1;
    running.qa_execution.as_mut().unwrap().status = QaExecutionStatusV1::Running;
    running
        .qa_execution
        .as_mut()
        .unwrap()
        .dispatch_task_revision = Some(revision);
    assert!(matches_pending_launch(&running, running.revision, &request));

    let mut stale = task.clone();
    stale.phase = crate::core::lifecycle::TaskPhase::Running;
    assert!(!matches_pending_launch(&stale, revision, &request));

    let mut stale = task.clone();
    stale.qa_execution.as_mut().unwrap().status = QaExecutionStatusV1::Blocked;
    assert!(!matches_pending_launch(&stale, revision, &request));

    let mut other_request = request.clone();
    other_request.qa_run_id.push_str("-replacement");
    let mut stale = task;
    stale.qa_execution.as_mut().unwrap().request = Some(other_request);
    assert!(!matches_pending_launch(&stale, revision, &request));
}

#[test]
// Production boundary: app-side preflight must not turn another process's
// Running request into Blocked while its worker owns the OS lock. The test
// uses two LifecycleStore handles and a real file lock; unit checks cannot
// prove cross-process lock behavior. No Docker or network; low setup cost.
fn active_running_worker_cannot_be_blocked_by_preflight() {
    let root = tempfile::tempdir().unwrap();
    let planning = crate::artifacts::planning_store::PlanningStore::legacy_embedded(
        uuid::Uuid::new_v4(),
        root.path(),
    );
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, queued_revision) = fixture::queued_task(&lifecycle);
    let lock_path = root.path().join(&request.qa_run_id).join("worker.lock");
    let (worker_lock, running) = worker::acquire_lock_and_claim(
        &lock_path,
        &lifecycle,
        &task_uid,
        queued_revision,
        &request,
    )
    .unwrap();

    let outcome = super::super::blocking::persist_block(
        &planning,
        root.path(),
        &running,
        &request,
        "runtime-qa-preflight-test-v1",
        "synthetic preflight failure".into(),
    )
    .unwrap();
    assert!(matches!(
        outcome,
        super::super::blocking::BlockOutcome::ActiveWorker
    ));
    assert_eq!(
        lifecycle
            .load_task(&task_uid)
            .unwrap()
            .qa_execution
            .unwrap()
            .status,
        QaExecutionStatusV1::Running
    );

    drop(worker_lock);
    let outcome = super::super::blocking::persist_block(
        &planning,
        root.path(),
        &running,
        &request,
        "runtime-qa-preflight-test-v1",
        "synthetic preflight failure".into(),
    )
    .unwrap();
    assert!(matches!(
        outcome,
        super::super::blocking::BlockOutcome::Persisted
    ));
    assert_eq!(
        lifecycle
            .load_task(&task_uid)
            .unwrap()
            .qa_execution
            .unwrap()
            .status,
        QaExecutionStatusV1::Blocked
    );
}
