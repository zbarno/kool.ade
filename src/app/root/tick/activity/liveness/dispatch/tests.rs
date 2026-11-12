use super::*;

/// Production boundary: liveness must distinguish a verified lease owner from
/// an unreadable lease store. Treating storage errors as an active worker can
/// hide a stranded TaskIntent. This checks the direct owner probe; app-level
/// recovery behavior is covered by the persisted liveness restart test.
/// Dependencies: local fixture files only. Setup cost: one app fixture.
#[test]
fn unreadable_running_task_lease_is_reported_instead_of_assumed_owned() {
    let mut app = crate::app::root::board_tests::fixture();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let fence_path = project.state.planning_store.root.join("task-fences");
    std::fs::write(&fence_path, b"not a directory").unwrap();
    let task = TaskRecord {
        schema_version: 1,
        task_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        feature_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        planning_item_uid: None,
        task_spec_sha256: None,
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        approved_requirement_ids: Vec::new(),
        phase: TaskPhase::Running,
        revision: 1,
        approved_plan_sha256: "a".repeat(64),
        implementation_sha256: None,
        reviewed_code_head_sha: None,
        reviewed_code_subject_sha256: None,
        code_review: None,
        review_history: Vec::new(),
        repair_attempts: 0,
        runtime_qa: None,
        qa_execution: None,
        spec_reconciliation: None,
        spec_reconciliation_blocker: None,
        spec_reconciliation_proposal_sha256: None,
        spec_reconciliation_spec_sha256: None,
        spec_reconciliation_proposal_json: None,
        pr_work_item_uid: None,
    };

    let result = has_dispatchable_task(project, &[&task]);
    assert!(
        result.is_err_and(|error| error.contains("Could not verify the running task lease")),
        "lease read failure must be distinguishable from a live owner"
    );
}
