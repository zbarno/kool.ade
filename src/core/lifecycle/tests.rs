use super::*;

#[path = "tests/spec_reconciliation.rs"]
mod spec_reconciliation;
#[path = "tests/store.rs"]
mod store;

fn id() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

fn review_prompt_manifest(
    role: crate::harness::prompt_catalog::PromptRole,
) -> crate::harness::prompt_catalog::PromptManifest {
    use crate::harness::prompt_catalog::RoleInputV1;
    use crate::harness::prompt_catalog::RolePayloadV1;
    crate::harness::prompt_catalog::render(&RoleInputV1 {
        role,
        workspace_uid: "fixture-workspace".into(),
        feature_uid: "fixture-feature".into(),
        task_uid: Some("fixture-task".into()),
        original_goal: "Review the approved artifact".into(),
        approved_spec_ref: None,
        approved_spec_hash: None,
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "review/fixture/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: "fixture-revision".into(),
            subject_hash: "a".repeat(64),
            author_run_id: "fixture-author".into(),
            reviewer_run_id: "fixture-reviewer".into(),
            author_session_id: Some("00000000-0000-4000-8000-000000000005".into()),
            reviewer_session_id: Some("00000000-0000-4000-8000-000000000006".into()),
            requirements_matrix: Vec::new(),
            explicit_exclusions: Vec::new(),
        },
    })
    .unwrap()
    .manifest
}

fn set_prompt_role(receipt: &mut GateReceipt, role: crate::harness::prompt_catalog::PromptRole) {
    let manifest = review_prompt_manifest(role);
    receipt.prompt_sha256 = Some(manifest.identity_sha256());
    receipt.prompt_manifest = Some(manifest);
}

fn receipt(actor: GateActor, decision: GateDecision, subject: &str) -> GateReceipt {
    let prompt_manifest = (actor == GateActor::IndependentReviewer)
        .then(|| review_prompt_manifest(crate::harness::prompt_catalog::PromptRole::CodeReviewer));
    GateReceipt {
        receipt_uid: id(),
        subject_uid: subject.into(),
        subject_sha256: "a".repeat(64),
        actor,
        decision,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture/evidence.json".into()],
        prompt_sha256: prompt_manifest
            .as_ref()
            .map(crate::harness::prompt_catalog::PromptManifest::identity_sha256),
        prompt_manifest,
        reviewer_run_id: (actor == GateActor::IndependentReviewer).then(id),
        author_run_id: (actor == GateActor::IndependentReviewer).then(id),
        reviewer_session_id: (actor == GateActor::IndependentReviewer).then(id),
        author_session_id: (actor == GateActor::IndependentReviewer).then(id),
    }
}

fn code_review_event(receipt: GateReceipt) -> TaskEvent {
    TaskEvent::CodeReview {
        receipt,
        reviewed_head_sha: "1".repeat(40),
        review_subject_sha256: "2".repeat(64),
    }
}

fn qa_request(task: &TaskRecord) -> crate::runtime_qa::QaRunRequestV1 {
    let review = task
        .code_review
        .as_ref()
        .expect("approved code review fixture");
    crate::runtime_qa::QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: id(),
        parent_task_uid: task.task_uid.clone(),
        feature_uid: task.feature_uid.clone(),
        code_head_sha: task.reviewed_code_head_sha.clone().unwrap(),
        build_artifact_sha256: "d".repeat(64),
        complete_diff_hash: task.implementation_sha256.clone().unwrap(),
        review_subject_sha256: task.reviewed_code_subject_sha256.clone().unwrap(),
        approved_spec_hash: task.approved_plan_sha256.clone(),
        requirement_ids: vec!["REQ-001".into()],
        acceptance_criteria: vec!["The application meets the approved behavior.".into()],
        relevant_platforms: vec![crate::runtime_qa::QaPlatform::Web],
        startup_recipe_ref: "recipe:fixture/v1".into(),
        startup_recipe_sha256: "8".repeat(64),
        execution_session_id: id(),
        driver_profile_version: "blocked-fixture-driver/v1".into(),
        test_data_fixture_ref: None,
        bounded_limits: crate::runtime_qa::QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 8,
            max_total_actions: 8,
            timeout_seconds: 60,
            max_evidence_bytes: 1024 * 1024,
            max_screenshots: 4,
        },
        author_run_id: review.author_run_id.clone().unwrap(),
        author_session_id: review.author_session_id.clone().unwrap(),
    }
}

fn completed_qa_event(task: &TaskRecord, request: &crate::runtime_qa::QaRunRequestV1) -> TaskEvent {
    let mut qa = receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &task.task_uid,
    );
    set_prompt_role(
        &mut qa,
        crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewer,
    );
    qa.subject_sha256 = request.complete_diff_hash.clone();
    qa.author_run_id = Some(request.author_run_id.clone());
    qa.author_session_id = Some(request.author_session_id.clone());
    let result_sha = "f".repeat(64);
    let evidence_sha = "a".repeat(64);
    qa.evidence_refs = vec![
        format!("runtime-qa:{}/result", request.qa_run_id),
        format!("runtime-qa:{}/evidence:{evidence_sha}", request.qa_run_id),
    ];
    TaskEvent::RuntimeQaCompleted {
        request_id: request.qa_run_id.clone(),
        reviewer_execution_id: qa.reviewer_run_id.clone().unwrap(),
        reviewer_session_id: qa.reviewer_session_id.clone().unwrap(),
        receipt: qa,
        result_sha256: result_sha,
        evidence_manifest_sha256: evidence_sha,
    }
}

fn feature() -> FeatureGoal {
    FeatureGoal {
        schema_version: 1,
        feature_uid: id(),
        original_intent: "Add a safe workflow".into(),
        draft: "A reviewed plan".into(),
        scope: vec!["workflow".into()],
        non_goals: vec!["deployment".into()],
        revision: 0,
        phase: FeaturePhase::Exploring,
        approved_plan_sha256: None,
        plan_snapshot_uid: None,
        plan_snapshot_sha256: None,
        unresolved_questions: Vec::new(),
        decision_refs: Vec::new(),
        review_refs: Vec::new(),
        readiness_evidence_refs: Vec::new(),
        task_uids: Vec::new(),
    }
}

#[path = "tests/feature.rs"]
mod feature;
#[path = "tests/receipts.rs"]
mod receipts;
#[path = "tests/task.rs"]
mod task;
