use super::lifecycle_parent_contains_planning_child;
use crate::core::lifecycle::{TaskPhase, TaskRecord};

fn task(task_uid: &str, planning_item_uid: &str) -> TaskRecord {
    TaskRecord {
        schema_version: 1,
        task_uid: task_uid.into(),
        feature_uid: "feature-lifecycle-uid".into(),
        planning_item_uid: Some(planning_item_uid.into()),
        task_spec_sha256: None,
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        approved_requirement_ids: Vec::new(),
        phase: TaskPhase::Ready,
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
    }
}

#[test]
fn completed_subtask_mapping_uses_planning_uids_and_lifecycle_ownership() {
    let mut parent = task("parent-lifecycle-uid", "parent-planning-uid");
    let mut child = task("child-lifecycle-uid", "child-planning-uid");
    parent.subtask_uids.push(child.task_uid.clone());
    child.parent_task_uid = Some(parent.task_uid.clone());

    assert!(lifecycle_parent_contains_planning_child(
        "child-planning-uid",
        "parent-planning-uid",
        &child,
        &parent,
    ));
    assert!(!lifecycle_parent_contains_planning_child(
        "child-lifecycle-uid",
        "parent-planning-uid",
        &child,
        &parent,
    ));
}
