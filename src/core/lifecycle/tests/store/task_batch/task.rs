use crate::core::lifecycle::{TaskPhase, TaskRecord};

pub(super) fn task(feature_uid: &str, plan_sha: &str, planning_uid: &str) -> TaskRecord {
    let namespace = uuid::Uuid::parse_str(feature_uid).unwrap();
    let spec_sha = format!("{}{}", plan_sha, planning_uid);
    let task_uid = uuid::Uuid::new_v5(&namespace, spec_sha.as_bytes())
        .hyphenated()
        .to_string();
    TaskRecord {
        schema_version: 1,
        task_uid,
        feature_uid: feature_uid.into(),
        planning_item_uid: Some(planning_uid.into()),
        task_spec_sha256: Some("d".repeat(64)),
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        approved_requirement_ids: Vec::new(),
        phase: TaskPhase::Ready,
        revision: 0,
        approved_plan_sha256: plan_sha.into(),
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
