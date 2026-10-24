use crate::core::lifecycle::{
    FeatureEvent, FeatureGoal, FeaturePhase, GateActor, GateDecision, GateReceipt, LifecycleStore,
    TaskEvent, TaskPhase, TaskRecord,
};
use crate::runtime_qa::{QaBoundedLimitsV1, QaPlatform, QaRunRequestV1};

fn id() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

fn receipt(
    actor: GateActor,
    decision: GateDecision,
    subject_uid: &str,
    role: crate::harness::prompt_catalog::PromptRole,
) -> GateReceipt {
    use crate::harness::prompt_catalog::{RoleInputV1, RolePayloadV1};
    let prompt_manifest = (actor == GateActor::IndependentReviewer).then(|| {
        crate::harness::prompt_catalog::render(&RoleInputV1 {
            role,
            workspace_uid: "dispatch-fixture-workspace".into(),
            feature_uid: subject_uid.into(),
            task_uid: Some(subject_uid.into()),
            original_goal: "Persist runtime QA worker failures".into(),
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
                author_session_id: Some(id()),
                reviewer_session_id: Some(id()),
                requirements_matrix: Vec::new(),
                explicit_exclusions: Vec::new(),
            },
        })
        .unwrap()
        .manifest
    });
    GateReceipt {
        receipt_uid: id(),
        subject_uid: subject_uid.into(),
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

pub(crate) fn queued_task(lifecycle: &LifecycleStore<'_>) -> (String, QaRunRequestV1, u64) {
    let mut feature = FeatureGoal {
        schema_version: 1,
        feature_uid: id(),
        original_intent: "Persist runtime QA worker failures".into(),
        draft: "A reviewed QA dispatch".into(),
        scope: vec!["runtime QA".into()],
        non_goals: Vec::new(),
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
    };
    let feature_uid = feature.feature_uid.clone();
    lifecycle.create_feature(&feature, &id()).unwrap();
    feature = lifecycle.load_feature(&feature_uid).unwrap();
    feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::ReadinessRecorded {
                evidence_refs: vec!["spec:acceptance".into()],
            },
        )
        .unwrap();
    let plan_sha = feature.plan_sha256();
    feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::PlanReady {
                plan_sha256: plan_sha.clone(),
            },
        )
        .unwrap();
    let mut user = receipt(
        GateActor::User,
        GateDecision::Approved,
        &feature_uid,
        crate::harness::prompt_catalog::PromptRole::PlanReviewer,
    );
    user.subject_sha256 = plan_sha.clone();
    feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::UserPlanDecision(user),
        )
        .unwrap();
    let mut plan_review = receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &feature_uid,
        crate::harness::prompt_catalog::PromptRole::PlanReviewer,
    );
    plan_review.subject_sha256 = plan_sha.clone();
    feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::PlanReview(plan_review),
        )
        .unwrap();
    lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::TaskGraphCreated {
                plan_sha256: plan_sha.clone(),
            },
        )
        .unwrap();

    let task_uid = id();
    lifecycle
        .create_task(
            &TaskRecord {
                schema_version: 1,
                task_uid: task_uid.clone(),
                feature_uid,
                planning_item_uid: None,
                task_spec_sha256: None,
                parent_task_uid: None,
                subtask_uids: Vec::new(),
                dependency_task_uids: Vec::new(),
                approved_requirement_ids: Vec::new(),
                phase: TaskPhase::Ready,
                revision: 0,
                approved_plan_sha256: plan_sha,
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
            },
            &id(),
        )
        .unwrap();
    let mut task = lifecycle
        .apply_task_event(&task_uid, 1, &id(), TaskEvent::Start)
        .unwrap();
    let code_sha = "c".repeat(64);
    task = lifecycle
        .apply_task_event(
            &task_uid,
            task.revision,
            &id(),
            TaskEvent::ImplementationFinished {
                code_sha256: code_sha.clone(),
            },
        )
        .unwrap();
    let mut code_review = receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &task_uid,
        crate::harness::prompt_catalog::PromptRole::CodeReviewer,
    );
    code_review.subject_sha256 = code_sha.clone();
    task = lifecycle
        .apply_task_event(
            &task_uid,
            task.revision,
            &id(),
            TaskEvent::CodeReview {
                receipt: code_review.clone(),
                reviewed_head_sha: "1".repeat(40),
                review_subject_sha256: "2".repeat(64),
            },
        )
        .unwrap();
    let request = QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: id(),
        parent_task_uid: task_uid.clone(),
        target_id: Some("target:fixture".into()),
        planning_item_uid: None,
        feature_uid: task.feature_uid.clone(),
        code_head_sha: "1".repeat(40),
        build_artifact_sha256: "d".repeat(64),
        complete_diff_hash: code_sha,
        review_subject_sha256: "2".repeat(64),
        approved_spec_hash: task.approved_plan_sha256.clone(),
        requirement_ids: vec!["REQ-001".into()],
        acceptance_criteria: vec!["Worker failure is durable".into()],
        relevant_platforms: vec![QaPlatform::Web],
        startup_recipe_ref: "recipe:fixture/v1".into(),
        startup_recipe_sha256: "8".repeat(64),
        execution_session_id: id(),
        driver_profile_version: "fixture-driver/v1".into(),
        test_data_fixture_ref: None,
        dependency_cache_refs: Vec::new(),
        bounded_limits: QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 8,
            max_total_actions: 8,
            timeout_seconds: 60,
            max_evidence_bytes: 1024 * 1024,
            max_screenshots: 4,
        },
        author_run_id: code_review.author_run_id.unwrap(),
        author_session_id: code_review.author_session_id.unwrap(),
    };
    task = lifecycle
        .apply_task_event(
            &task_uid,
            task.revision,
            &id(),
            TaskEvent::RuntimeQaUnavailable {
                reason: "recipe is being prepared".into(),
            },
        )
        .unwrap();
    task = lifecycle
        .apply_task_event(
            &task_uid,
            task.revision,
            &id(),
            TaskEvent::RuntimeQaRequested(request.clone()),
        )
        .unwrap();
    (task_uid, request, task.revision)
}
