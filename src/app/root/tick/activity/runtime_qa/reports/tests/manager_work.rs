use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::planning_work::{self, WorkKind, WorkStatus},
    runtime_qa::RuntimeQaStore,
};

/// Production boundary: persisted blocking QA evidence becomes durable Manager
/// work on the Manager route. File-backed stores prove restart and replay
/// behavior; no Docker, provider, or network dependency is needed.
#[test]
fn blocking_qa_finding_creates_one_manager_repair_proposal() {
    let root = tempfile::tempdir().unwrap();
    let planning = PlanningStore::new(uuid::Uuid::new_v4(), root.path(), StoreMode::ManagedLocal);
    let task = super::task();
    let request = super::request(&task);
    let qa = RuntimeQaStore::new(root.path().join("qa"));
    super::receipt_order::save_failed_report_with_finding(&qa, &task.task_uid, &request, true);
    let (_, _, _, result) = qa.load_run(&request.qa_run_id).unwrap();

    let mut work = Vec::new();
    let mut baseline = planning.revision().unwrap();
    super::super::manager_work::persist(
        &planning,
        &mut work,
        &mut baseline,
        &task,
        &request,
        &result,
    )
    .unwrap();
    let saved = planning_work::load(&planning).unwrap();
    assert_eq!(saved.len(), 1);
    let proposal = &saved[0];
    assert_eq!(proposal.kind, WorkKind::QaFindingDisposition);
    assert_eq!(proposal.status, WorkStatus::Todo);
    assert_eq!(
        proposal.kind.harness_route_category(),
        crate::persistence::harness_settings::MANAGER
    );
    assert_eq!(proposal.parent_uid.as_deref(), Some(task.task_uid.as_str()));
    assert!(proposal.request.contains(&result.findings[0].finding_uid));
    assert!(proposal.detail.contains(&request.qa_run_id));
    assert!(proposal.detail.contains(&request.code_head_sha));
    assert!(proposal.detail.contains(&result.findings[0].smallest_fix));

    super::super::manager_work::persist(
        &planning,
        &mut work,
        &mut baseline,
        &task,
        &request,
        &result,
    )
    .unwrap();
    assert_eq!(planning_work::load(&planning).unwrap().len(), 1);
}
