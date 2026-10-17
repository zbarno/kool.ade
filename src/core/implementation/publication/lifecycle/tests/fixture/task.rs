use super::id;
use crate::{
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, QaExecutionStateV1, QaExecutionStatusV1, TaskPhase,
        TaskRecord,
    },
    harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render},
    runtime_qa::{QaBoundedLimitsV1, QaPlatform, QaRunRequestV1},
};

pub(super) fn completed(feature_uid: &str, work_uid: &str, head: &str) -> TaskRecord {
    let task_uid = id();
    let review = code_review(feature_uid, &task_uid);
    let request = QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: id(),
        parent_task_uid: task_uid.clone(),
        feature_uid: feature_uid.into(),
        code_head_sha: head.into(),
        build_artifact_sha256: "e".repeat(64),
        complete_diff_hash: "d".repeat(64),
        review_subject_sha256: "f".repeat(64),
        approved_spec_hash: "c".repeat(64),
        requirement_ids: vec!["REQ-001".into()],
        acceptance_criteria: vec!["Publish only reviewed work".into()],
        relevant_platforms: vec![QaPlatform::Web],
        startup_recipe_ref: "fixture:recipe/v1".into(),
        startup_recipe_sha256: "1".repeat(64),
        execution_session_id: id(),
        driver_profile_version: "fixture-driver/v1".into(),
        test_data_fixture_ref: None,
        bounded_limits: QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 1,
            max_total_actions: 1,
            timeout_seconds: 30,
            max_evidence_bytes: 1024,
            max_screenshots: 1,
        },
        author_run_id: review.author_run_id.clone().unwrap(),
        author_session_id: review.author_session_id.clone().unwrap(),
    };
    TaskRecord {
        schema_version: 1,
        task_uid: task_uid.clone(),
        feature_uid: feature_uid.into(),
        planning_item_uid: Some(id()),
        task_spec_sha256: Some("b".repeat(64)),
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        approved_requirement_ids: vec!["REQ-001".into()],
        phase: TaskPhase::Done,
        revision: 7,
        approved_plan_sha256: "c".repeat(64),
        implementation_sha256: Some("d".repeat(64)),
        reviewed_code_head_sha: Some(head.into()),
        reviewed_code_subject_sha256: Some("f".repeat(64)),
        code_review: Some(review.clone()),
        review_history: vec![review],
        repair_attempts: 0,
        runtime_qa: Some(policy_receipt(&task_uid)),
        qa_execution: Some(QaExecutionStateV1 {
            schema_version: 1,
            status: QaExecutionStatusV1::Completed,
            request: Some(request),
            blocked_reason: None,
            result_sha256: Some("2".repeat(64)),
            evidence_manifest_sha256: Some("3".repeat(64)),
            reviewer_execution_id: Some(id()),
            reviewer_session_id: Some(id()),
        }),
        spec_reconciliation: Some(policy_receipt(&task_uid)),
        spec_reconciliation_blocker: None,
        spec_reconciliation_proposal_sha256: None,
        spec_reconciliation_spec_sha256: None,
        spec_reconciliation_proposal_json: None,
        pr_work_item_uid: Some(work_uid.into()),
    }
}

fn policy_receipt(task_uid: &str) -> GateReceipt {
    GateReceipt {
        receipt_uid: id(),
        subject_uid: task_uid.into(),
        subject_sha256: "d".repeat(64),
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture:accepted-gate-contract/v1".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    }
}

fn code_review(feature_uid: &str, task_uid: &str) -> GateReceipt {
    let author_run = id();
    let reviewer_run = id();
    let author_session = id();
    let reviewer_session = id();
    let role = PromptRole::CodeReviewer;
    let manifest = render(&RoleInputV1 {
        role,
        workspace_uid: id(),
        feature_uid: feature_uid.into(),
        task_uid: Some(task_uid.into()),
        original_goal: "Publish only reviewed work".into(),
        approved_spec_ref: None,
        approved_spec_hash: None,
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "fixture:publication/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: "1".into(),
            subject_hash: "d".repeat(64),
            author_run_id: author_run.clone(),
            reviewer_run_id: reviewer_run.clone(),
            author_session_id: Some(author_session.clone()),
            reviewer_session_id: Some(reviewer_session.clone()),
            requirements_matrix: vec!["Publish only reviewed work".into()],
            explicit_exclusions: Vec::new(),
        },
    })
    .unwrap()
    .manifest;
    GateReceipt {
        receipt_uid: id(),
        subject_uid: task_uid.into(),
        subject_sha256: "d".repeat(64),
        actor: GateActor::IndependentReviewer,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture:accepted-code-review/v1".into()],
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(reviewer_run),
        author_run_id: Some(author_run),
        reviewer_session_id: Some(reviewer_session),
        author_session_id: Some(author_session),
    }
}
