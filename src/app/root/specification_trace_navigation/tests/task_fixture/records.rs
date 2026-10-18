use crate::artifacts::planning_store::PlanningStore;
use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt, TaskPhase, TaskRecord};
#[path = "records/pr_record.rs"]
mod pr_record;
pub(in crate::app::root::specification_trace_navigation::tests) use pr_record::save_pr_state;

pub(super) fn code_review(
    store: &PlanningStore,
    feature_uid: &str,
    snapshot_uid: &str,
    task_uid: &str,
    approved_hash: &str,
    diff_hash: &str,
) -> GateReceipt {
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render};
    let author_run = uid();
    let reviewer_run = uid();
    let author_session = uid();
    let reviewer_session = uid();
    let role = PromptRole::CodeReviewer;
    let manifest = render(&RoleInputV1 {
        role,
        workspace_uid: store.project_id.hyphenated().to_string(),
        feature_uid: feature_uid.into(),
        task_uid: Some(task_uid.into()),
        original_goal: "Verify trace navigation fixture".into(),
        approved_spec_ref: Some(snapshot_uid.into()),
        approved_spec_hash: Some(approved_hash.into()),
        decision_refs: Vec::new(),
        evidence_manifest: vec![crate::harness::prompt_catalog::EvidenceItem {
            id: "fixture-review-diff".into(),
            workspace_uid: store.project_id.hyphenated().to_string(),
            origin: crate::harness::prompt_catalog::EvidenceOrigin::Repository,
            confidentiality:
                crate::harness::prompt_catalog::EvidenceConfidentiality::WorkspacePrivate,
            source: "fixture/review-diff".into(),
            content: "synthetic review evidence".into(),
        }],
        execution_policy_ref: "trace-navigation-fixture/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: "fixture-1".into(),
            subject_hash: diff_hash.into(),
            author_run_id: author_run.clone(),
            reviewer_run_id: reviewer_run.clone(),
            author_session_id: Some(author_session.clone()),
            reviewer_session_id: Some(reviewer_session.clone()),
            requirements_matrix: vec!["Each reference opens its persisted target".into()],
            explicit_exclusions: Vec::new(),
        },
    })
    .unwrap()
    .manifest;
    GateReceipt {
        receipt_uid: uid(),
        subject_uid: task_uid.into(),
        subject_sha256: diff_hash.into(),
        actor: GateActor::IndependentReviewer,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture:review-diff".into()],
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(reviewer_run),
        author_run_id: Some(author_run),
        reviewer_session_id: Some(reviewer_session),
        author_session_id: Some(author_session),
    }
}

#[allow(clippy::too_many_arguments)] // Test fixture keeps the bound request inputs visible.
pub(super) fn qa_request(
    feature_uid: &str,
    task_uid: &str,
    requirement_id: &str,
    plan_hash: &str,
    diff_hash: &str,
    head: &str,
    review_subject: &str,
    author_run: &str,
    author_session: &str,
) -> crate::runtime_qa::QaRunRequestV1 {
    crate::runtime_qa::QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uid(),
        parent_task_uid: task_uid.into(),
        planning_item_uid: None,
        feature_uid: feature_uid.into(),
        code_head_sha: head.into(),
        build_artifact_sha256: "d".repeat(64),
        complete_diff_hash: diff_hash.into(),
        review_subject_sha256: review_subject.into(),
        approved_spec_hash: plan_hash.into(),
        requirement_ids: vec![requirement_id.into()],
        acceptance_criteria: vec!["Trace link opens the target".into()],
        relevant_platforms: vec![crate::runtime_qa::QaPlatform::Web],
        startup_recipe_ref: "recipe:trace-fixture/v1".into(),
        startup_recipe_sha256: "e".repeat(64),
        execution_session_id: uid(),
        driver_profile_version: "fixture-driver/v1".into(),
        test_data_fixture_ref: None,
        dependency_cache_refs: Vec::new(),
        bounded_limits: crate::runtime_qa::QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 1,
            max_total_actions: 1,
            timeout_seconds: 30,
            max_evidence_bytes: 1024,
            max_screenshots: 1,
        },
        author_run_id: author_run.into(),
        author_session_id: author_session.into(),
    }
}

pub(super) fn qa_receipt(task_uid: &str, diff_hash: &str) -> GateReceipt {
    GateReceipt {
        receipt_uid: uid(),
        subject_uid: task_uid.into(),
        subject_sha256: diff_hash.into(),
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture:runtime-qa".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    }
}

#[allow(clippy::too_many_arguments)] // Test fixture enumerates all persisted trace bindings.
pub(super) fn task_record(
    feature_uid: &str,
    task_uid: &str,
    requirement_id: &str,
    plan_hash: &str,
    diff_hash: &str,
    head: &str,
    review_subject: &str,
    review: GateReceipt,
    qa: GateReceipt,
    request: crate::runtime_qa::QaRunRequestV1,
    work_uid: &str,
) -> TaskRecord {
    TaskRecord {
        schema_version: 1,
        task_uid: task_uid.into(),
        feature_uid: feature_uid.into(),
        planning_item_uid: Some(uid()),
        task_spec_sha256: Some("f".repeat(64)),
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        approved_requirement_ids: vec![requirement_id.into()],
        phase: TaskPhase::Ready,
        revision: 0,
        approved_plan_sha256: plan_hash.into(),
        implementation_sha256: Some(diff_hash.into()),
        reviewed_code_head_sha: Some(head.into()),
        reviewed_code_subject_sha256: Some(review_subject.into()),
        code_review: Some(review),
        review_history: Vec::new(),
        repair_attempts: 0,
        runtime_qa: Some(qa),
        qa_execution: Some(crate::core::lifecycle::QaExecutionStateV1 {
            schema_version: 1,
            status: crate::core::lifecycle::QaExecutionStatusV1::Completed,
            request: Some(request),
            blocked_reason: None,
            result_sha256: Some("1".repeat(64)),
            evidence_manifest_sha256: Some("2".repeat(64)),
            reviewer_execution_id: Some(uid()),
            reviewer_session_id: Some(uid()),
        }),
        spec_reconciliation: None,
        spec_reconciliation_blocker: None,
        spec_reconciliation_proposal_sha256: None,
        spec_reconciliation_spec_sha256: None,
        spec_reconciliation_proposal_json: None,
        pr_work_item_uid: Some(work_uid.into()),
    }
}

pub(super) fn followup_task(task: &TaskRecord, followup_uid: &str) -> TaskRecord {
    let mut followup = task.clone();
    followup.task_uid = followup_uid.into();
    followup.planning_item_uid = Some(uid());
    followup.implementation_sha256 = None;
    followup.reviewed_code_head_sha = None;
    followup.reviewed_code_subject_sha256 = None;
    followup.code_review = None;
    followup.runtime_qa = None;
    followup.qa_execution = None;
    followup.pr_work_item_uid = None;
    followup
}

pub(super) fn save_decision(
    store: &PlanningStore,
    feature_uid: &str,
    requirement_id: &str,
) -> String {
    let id = uid();
    let question = crate::core::planning::QuestionV1 {
        uid: id.clone(),
        feature_uid: feature_uid.into(),
        plan_revision: 1,
        decision_subject: "Trace navigation target".into(),
        decision_domain: "traceability".into(),
        origin_requirement_ids: vec![requirement_id.into()],
        category: crate::core::planning::QuestionCategory::Testing,
        priority: crate::core::planning::QuestionPriority::Important,
        question: "Which target should this reference open?".into(),
        reason: "The trace must open a persisted record.".into(),
        options: vec![crate::core::planning::QuestionOption {
            id: "record".into(),
            label: "Open record".into(),
            tradeoffs: "Keeps the record auditable.".into(),
        }],
        recommended_option_id: Some("record".into()),
        no_answer_impact: "Target remains ambiguous.".into(),
        evidence_refs: vec![format!("feature:{feature_uid}")],
        assumption_if_deferred: None,
        deferred: None,
        depends_on_question_ids: Vec::new(),
        status: crate::core::planning::QuestionStatus::Answered,
        answer_text: Some("Open the persisted target.".into()),
        selected_option: Some("record".into()),
        answered_by: Some("Fixture operator".into()),
        answered_at_ms: Some(1),
        deferred_by: None,
        deferred_at_ms: None,
        revision: 0,
    };
    crate::core::planning::QuestionStore::new(store)
        .save(&question, 0)
        .unwrap();
    id
}

pub(super) fn save_readiness_test(store: &PlanningStore, feature_uid: &str, test_ref: &str) {
    let questions = crate::core::planning::QuestionStore::new(store);
    let (mut readiness, revision) = questions.load_readiness(feature_uid).unwrap().unwrap();
    readiness.acceptance_tests[0].test_ref = test_ref.into();
    readiness.revision += 1;
    questions.save_readiness(&readiness, revision).unwrap();
}

fn uid() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}
