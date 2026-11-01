use super::*;

pub(crate) fn save_valid_gate_receipt(store: &RuntimeQaStore, request: &QaRunRequestV1) {
    use crate::{
        core::lifecycle::{GateActor, GateDecision, GateReceipt},
        harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render},
    };

    let (_, _, evidence, result) = store.load_run(&request.qa_run_id).unwrap();
    let role = PromptRole::RuntimeQaReviewerV3;
    let prompt = render(&RoleInputV1 {
        role,
        workspace_uid: request.feature_uid.clone(),
        feature_uid: request.feature_uid.clone(),
        task_uid: Some(request.parent_task_uid.clone()),
        original_goal: "Review the persisted runtime QA report".into(),
        approved_spec_ref: Some("approved-spec".into()),
        approved_spec_hash: Some(request.approved_spec_hash.clone()),
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "read-only isolated reviewer".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: request.code_head_sha.clone(),
            subject_hash: request.complete_diff_hash.clone(),
            author_run_id: request.author_run_id.clone(),
            reviewer_run_id: result.reviewer_execution_id.clone(),
            author_session_id: Some(request.author_session_id.clone()),
            reviewer_session_id: Some(result.reviewer_session_id.clone()),
            requirements_matrix: request.acceptance_criteria.clone(),
            explicit_exclusions: Vec::new(),
        },
    })
    .unwrap()
    .manifest;
    let receipt = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: request.parent_task_uid.clone(),
        subject_sha256: request.complete_diff_hash.clone(),
        actor: GateActor::IndependentReviewer,
        decision: if result.outcome == QaOutcome::Pass {
            GateDecision::Approved
        } else {
            GateDecision::ChangesRequested
        },
        recorded_at_unix_ms: 1,
        evidence_refs: vec![
            format!("runtime-qa:{}/result", request.qa_run_id),
            format!(
                "runtime-qa:{}/evidence:{}",
                request.qa_run_id,
                evidence.sha256().unwrap()
            ),
            format!(
                "runtime-qa:{}/scenarios:{}",
                request.qa_run_id, result.scenario_set_sha256
            ),
        ],
        prompt_sha256: Some(prompt.identity_sha256()),
        prompt_manifest: Some(prompt),
        reviewer_run_id: Some(result.reviewer_execution_id.clone()),
        author_run_id: Some(request.author_run_id.clone()),
        reviewer_session_id: Some(result.reviewer_session_id.clone()),
        author_session_id: Some(request.author_session_id.clone()),
    };
    store.save_gate_receipt(&receipt).unwrap();
}
