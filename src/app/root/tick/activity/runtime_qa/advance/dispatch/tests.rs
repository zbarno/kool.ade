use super::*;
use crate::core::lifecycle::QaExecutionStatusV1;

#[path = "tests/fixture.rs"]
mod fixture;

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
    let (task_uid, request, revision) = fixture::queued_task(&lifecycle);
    let run_id = request.qa_run_id.clone();
    let result = start_worker_or_block(&lifecycle, &task_uid, Ok(revision), &run_id, || {
        Err("simulated worker thread spawn failure".into())
    });
    assert!(
        matches!(result, Err((reason, None)) if reason == "simulated worker thread spawn failure")
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
