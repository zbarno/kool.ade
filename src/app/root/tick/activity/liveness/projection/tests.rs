use super::{phase_for_pr_status, task_phases};
use crate::core::lifecycle::{
    LifecycleStore, PullRequestPreparationStatus as PrStatus, TaskPhase, TaskRecord,
};

#[test]
fn persisted_pr_status_is_projected_to_the_liveness_task_phase() {
    for (status, expected) in [
        (PrStatus::AwaitingAuthorization, TaskPhase::PrPending),
        (PrStatus::Publishing, TaskPhase::PrPending),
        (PrStatus::NeedsAttention, TaskPhase::Blocked),
        (PrStatus::ClosedUnmerged, TaskPhase::Blocked),
        (PrStatus::Opened, TaskPhase::PrOpened),
        (PrStatus::Merged, TaskPhase::PrMerged),
    ] {
        assert_eq!(phase_for_pr_status(status), expected, "{status:?}");
    }
}

#[test]
fn missing_persisted_pr_state_does_not_fall_back_to_the_task_phase() {
    let directory = tempfile::tempdir().unwrap();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        directory.path(),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let lifecycle = LifecycleStore::new(&store);
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
        phase: TaskPhase::Done,
        revision: 1,
        approved_plan_sha256: "a".repeat(64),
        implementation_sha256: Some("b".repeat(64)),
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
        pr_work_item_uid: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
    };

    assert!(
        task_phases(&lifecycle, &[&task]).is_err(),
        "a missing publication record must not be treated as TaskPhase::Done"
    );
}
