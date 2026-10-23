use super::super::super::super::blocking::{self, BlockOutcome};

use super::super::fixture;

use super::{save_failed_report_without_receipt, save_infrastructure_report};

use crate::runtime_qa::*;

#[test]
// Production boundary: a valid report persisted just before a preflight blocker
// acquires the run lock must be applied before the blocker can mutate lifecycle.
// This uses file-backed stores and a real lock; no Docker or network setup.
fn completed_report_wins_over_a_late_preflight_blocker() {
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
    save_infrastructure_report(&RuntimeQaStore::new(&qa_root), &task_uid, &request);

    let outcome = blocking::persist_block(
        &planning,
        &qa_root,
        &running,
        &request,
        "runtime-qa-preflight-test-v1",
        "synthetic preflight failure".into(),
    )
    .unwrap();

    assert!(matches!(outcome, BlockOutcome::ReportPresent(_)));
    let execution = lifecycle
        .load_task(&task_uid)
        .unwrap()
        .qa_execution
        .unwrap();
    assert_eq!(
        execution.status,
        crate::core::lifecycle::QaExecutionStatusV1::Blocked
    );
    assert_eq!(
        execution.blocked_reason.as_deref(),
        Some("runtime QA outcome: InfrastructureBlocked")
    );
    assert_ne!(
        execution.blocked_reason.as_deref(),
        Some("synthetic preflight failure")
    );
}

#[test]
// Production boundary: worker recovery checks the same validated report while
// holding its run lock, so a completed execution cannot be launched twice.
// This uses a file-backed store and lock; no Docker or network setup.
fn completed_report_prevents_running_worker_restart() {
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
    save_infrastructure_report(&RuntimeQaStore::new(&qa_root), &task_uid, &request);
    let lock_path = qa_root.join(&request.qa_run_id).join("worker.lock");

    let outcome = super::super::super::super::worker::acquire_lock_and_claim(
        &lock_path,
        &lifecycle,
        &task_uid,
        running.revision,
        &request,
    );

    assert!(matches!(
        outcome,
        Err(super::super::super::super::worker::StartError::ReportPresent)
    ));
    assert_eq!(
        lifecycle
            .load_task(&task_uid)
            .unwrap()
            .qa_execution
            .unwrap()
            .status,
        crate::core::lifecycle::QaExecutionStatusV1::Running
    );
}

#[test]
fn mismatched_report_does_not_suppress_a_preflight_blocker() {
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
    let mut mismatched = request.clone();
    mismatched.complete_diff_hash = "9".repeat(64);
    save_infrastructure_report(&RuntimeQaStore::new(&qa_root), &task_uid, &mismatched);

    let outcome = blocking::persist_block(
        &planning,
        &qa_root,
        &running,
        &request,
        "runtime-qa-preflight-test-v1",
        "synthetic preflight failure".into(),
    )
    .unwrap();

    assert!(matches!(outcome, BlockOutcome::Persisted));
    assert_eq!(
        lifecycle
            .load_task(&task_uid)
            .unwrap()
            .qa_execution
            .unwrap()
            .blocked_reason
            .as_deref(),
        Some("synthetic preflight failure")
    );
}

#[test]
fn completed_report_without_gate_receipt_does_not_suppress_a_blocker() {
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
    save_failed_report_without_receipt(&RuntimeQaStore::new(&qa_root), &task_uid, &request);

    let outcome = blocking::persist_block(
        &planning,
        &qa_root,
        &running,
        &request,
        "runtime-qa-preflight-test-v1",
        "report has no reviewer receipt".into(),
    )
    .unwrap();

    assert!(matches!(outcome, BlockOutcome::Persisted));
    assert_eq!(
        lifecycle
            .load_task(&task_uid)
            .unwrap()
            .qa_execution
            .unwrap()
            .blocked_reason
            .as_deref(),
        Some("report has no reviewer receipt")
    );
}
