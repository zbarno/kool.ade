use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt, TaskEvent, TaskPhase};
use crate::core::self_improvement::SelfImprovementPhaseV1 as Phase;
use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render};

pub(super) fn verify_reviewer_and_qa_projection(
    app: &mut crate::app::root::KooladeApp,
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    runs: &crate::core::self_improvement::SelfImprovementStore<'_>,
    run_uid: &str,
    task_uid: &str,
) {
    let mut task = lifecycle.load_task(task_uid).unwrap();
    task = apply_task(lifecycle, &task, TaskEvent::Start);
    let implementation_sha256 = "c".repeat(64);
    task = apply_task(
        lifecycle,
        &task,
        TaskEvent::ImplementationFinished {
            code_sha256: implementation_sha256.clone(),
        },
    );
    let head_sha = "d".repeat(40);
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    project
        .implementation_states
        .values_mut()
        .find(|state| state.task_uid.as_deref() == Some(task_uid))
        .expect("validated task clone implementation")
        .verified_head = Some(head_sha.clone());

    super::synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
    let run = runs.load(run_uid).unwrap();
    assert_eq!(run.phase, Phase::Reviewing);
    assert_eq!(
        run.implementation_head_sha.as_deref(),
        Some(head_sha.as_str())
    );
    assert!(run.code_review_uids.is_empty());

    let code_review = reviewer_receipt(
        PromptRole::CodeReviewer,
        &task,
        &implementation_sha256,
        &head_sha,
        uuid::Uuid::new_v4().hyphenated().to_string(),
        uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let code_author_run = code_review.author_run_id.clone().unwrap();
    let code_author_session = code_review.author_session_id.clone().unwrap();
    task = apply_task(
        lifecycle,
        &task,
        TaskEvent::CodeReview {
            receipt: code_review,
            reviewed_head_sha: head_sha.clone(),
            review_subject_sha256: "e".repeat(64),
        },
    );
    assert_eq!(task.phase, TaskPhase::AwaitingRuntimeQa);
    super::synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
    let run = runs.load(run_uid).unwrap();
    assert_eq!(run.phase, Phase::Qa);
    assert_eq!(run.code_review_uids.len(), 1);
    assert!(run.runtime_qa_uids.is_empty());

    let request = crate::runtime_qa::QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        parent_task_uid: task.task_uid.clone(),
        planning_item_uid: Some(task.task_uid.clone()),
        feature_uid: task.feature_uid.clone(),
        code_head_sha: head_sha.clone(),
        build_artifact_sha256: "f".repeat(64),
        complete_diff_hash: implementation_sha256.clone(),
        review_subject_sha256: "e".repeat(64),
        approved_spec_hash: task.approved_plan_sha256.clone(),
        requirement_ids: vec!["REQ-001".into()],
        acceptance_criteria: vec!["The reviewed behavior is ready.".into()],
        relevant_platforms: vec![crate::runtime_qa::QaPlatform::Web],
        startup_recipe_ref: "recipe:fixture/v1".into(),
        startup_recipe_sha256: "8".repeat(64),
        execution_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        driver_profile_version: "fixture-driver/v1".into(),
        test_data_fixture_ref: None,
        dependency_cache_refs: Vec::new(),
        bounded_limits: crate::runtime_qa::QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 8,
            max_total_actions: 8,
            timeout_seconds: 60,
            max_evidence_bytes: 1024 * 1024,
            max_screenshots: 4,
        },
        author_run_id: code_author_run.clone(),
        author_session_id: code_author_session.clone(),
    };
    task = apply_task(
        lifecycle,
        &task,
        TaskEvent::RuntimeQaRequested(request.clone()),
    );
    let evidence_sha256 = "a".repeat(64);
    let mut runtime_qa = reviewer_receipt(
        PromptRole::RuntimeQaReviewer,
        &task,
        &implementation_sha256,
        &head_sha,
        code_author_run,
        code_author_session,
    );
    runtime_qa.evidence_refs = vec![
        format!("runtime-qa:{}/result", request.qa_run_id),
        format!(
            "runtime-qa:{}/evidence:{evidence_sha256}",
            request.qa_run_id
        ),
    ];
    let qa_reviewer_run = runtime_qa.reviewer_run_id.clone().unwrap();
    let qa_reviewer_session = runtime_qa.reviewer_session_id.clone().unwrap();
    task = apply_task(
        lifecycle,
        &task,
        TaskEvent::RuntimeQaCompleted {
            request_id: request.qa_run_id.clone(),
            result_sha256: "b".repeat(64),
            evidence_manifest_sha256: evidence_sha256,
            reviewer_execution_id: qa_reviewer_run,
            reviewer_session_id: qa_reviewer_session,
            receipt: runtime_qa,
        },
    );
    assert_eq!(task.phase, TaskPhase::AwaitingSpecReconciliation);
    super::synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
    let run = runs.load(run_uid).unwrap();
    assert_eq!(run.phase, Phase::SpecSync);
    assert_eq!(run.runtime_qa_uids.len(), 1);
    assert!(run.spec_receipt_uid.is_none());
    super::assert_app_idle(app);
}

fn apply_task(
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    task: &crate::core::lifecycle::TaskRecord,
    event: TaskEvent,
) -> crate::core::lifecycle::TaskRecord {
    lifecycle
        .apply_task_event(
            &task.task_uid,
            task.revision,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            event,
        )
        .unwrap()
}

fn reviewer_receipt(
    role: PromptRole,
    task: &crate::core::lifecycle::TaskRecord,
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
