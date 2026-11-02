use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt, TaskEvent, TaskRecord};
use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render};

pub(super) fn apply_task(
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    task: &TaskRecord,
    event: TaskEvent,
) -> TaskRecord {
    lifecycle
        .apply_task_event(
            &task.task_uid,
            task.revision,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            event,
        )
        .unwrap()
}

pub(super) fn reviewer_receipt(
    role: PromptRole,
    task: &TaskRecord,
    subject_sha256: &str,
    subject_revision: &str,
    author_run_id: String,
    author_session_id: String,
) -> GateReceipt {
    let reviewer_run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let manifest = render(&RoleInputV1 {
        role,
        workspace_uid: "fixture-workspace".into(),
        feature_uid: task.feature_uid.clone(),
        task_uid: Some(task.task_uid.clone()),
        original_goal: "Exercise self-improvement review synchronization".into(),
        approved_spec_ref: Some(task.feature_uid.clone()),
        approved_spec_hash: Some(task.approved_plan_sha256.clone()),
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "self-improvement-review-fixture/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: subject_revision.into(),
            subject_hash: subject_sha256.into(),
            author_run_id: author_run_id.clone(),
            reviewer_run_id: reviewer_run_id.clone(),
            author_session_id: Some(author_session_id.clone()),
            reviewer_session_id: Some(reviewer_session_id.clone()),
            requirements_matrix: vec!["REQ-001".into()],
            explicit_exclusions: Vec::new(),
        },
    })
    .unwrap()
    .manifest;
    GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: task.task_uid.clone(),
        subject_sha256: subject_sha256.into(),
        actor: GateActor::IndependentReviewer,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture/independent-review".into()],
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(reviewer_run_id),
        author_run_id: Some(author_run_id),
        reviewer_session_id: Some(reviewer_session_id),
        author_session_id: Some(author_session_id),
    }
}
