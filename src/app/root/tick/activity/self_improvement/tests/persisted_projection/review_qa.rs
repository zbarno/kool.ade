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
    let mut current_qa = reviewer_receipt(
        PromptRole::RuntimeQaReviewerV3,
        &task,
        &implementation_sha256,
        &head_sha,
        uuid::Uuid::new_v4().hyphenated().to_string(),
        uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let current_qa_run = uuid::Uuid::new_v4().hyphenated().to_string();
    current_qa.evidence_refs = vec![
        format!("runtime-qa:{current_qa_run}/result"),
        format!("runtime-qa:{current_qa_run}/evidence:{}", "a".repeat(64)),
        format!("runtime-qa:{current_qa_run}/scenarios:{}", "b".repeat(64)),
    ];
    let mut legacy_v3_qa = current_qa.clone();
    legacy_v3_qa
        .evidence_refs
        .retain(|reference| !reference.contains("/scenarios:"));
    let legacy_qa = reviewer_receipt(
        PromptRole::RuntimeQaReviewerV2,
        &task,
        &implementation_sha256,
        &head_sha,
        uuid::Uuid::new_v4().hyphenated().to_string(),
        uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    // Replay remains compatible with these old receipts, but they cannot
    // authorize a new self-improvement QA-to-spec transition.
    let current_qa_pass = super::super::super::task_evidence::current_runtime_qa_pass;
    assert!(current_qa.validate().is_ok());
    assert!(current_qa_pass(&current_qa, Some(&implementation_sha256)));
    assert!(legacy_v3_qa.validate().is_ok());
    assert!(!current_qa_pass(
        &legacy_v3_qa,
        Some(&implementation_sha256)
    ));
    assert!(!current_qa_pass(&legacy_qa, Some(&implementation_sha256)));
    assert!(!current_qa_pass(&current_qa, Some(&"f".repeat(64))));
    let not_applicable = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: task.task_uid.clone(),
        subject_sha256: implementation_sha256.clone(),
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::NotApplicable,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture:legacy-applicability".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    assert!(not_applicable.validate().is_ok());
    assert!(!current_qa_pass(
        &not_applicable,
        Some(&implementation_sha256)
    ));
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
        target_id: Some("fixture-target".into()),
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
    let qa_root = tempfile::tempdir().unwrap();
    task = qa_completion::run_and_record(lifecycle, &task, &request, qa_root.path());
    assert_eq!(task.phase, TaskPhase::AwaitingSpecReconciliation);
    super::synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
    let run = runs.load(run_uid).unwrap();
    assert_eq!(run.phase, Phase::SpecSync);
    assert_eq!(run.runtime_qa_uids.len(), 1);
    assert!(run.spec_receipt_uid.is_none());

    // Production boundary: pass a deterministic no-change SpecReconciler
    // result through the app's worker-result polling and completion handler.
    // A reducer-only projection test cannot prove that the real completion
    // handler persists its application-policy receipt and creates a separate
    // PR-preparation work item without authorizing publication. This uses the
    // local fixture stores and synthetic reviewer/QA receipts above; there is
    // no provider, Docker, or network dependency, with one headless app setup.
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    crate::app::root::spec_reconciliation::test_support::complete(
        project,
        task_uid,
        crate::harness::prompt_catalog::SpecDeltaV1 { deltas: Vec::new() },
        false,
    );
    let completed = lifecycle.load_task(task_uid).unwrap();
    assert_eq!(
        completed.phase,
        TaskPhase::Done,
        "specification completion was blocked: {:?}",
        project.activity.pending
    );
    let work_uid = completed
        .pr_work_item_uid
        .as_deref()
        .expect("spec reconciliation creates a scoped PR-preparation item");
    let work = lifecycle.load_pr_preparation(work_uid).unwrap();
    assert_eq!(work.task_uid, task_uid);
    assert_eq!(work.included_task_uids, vec![task_uid.to_owned()]);
    assert_eq!(
        work.status,
        crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization
    );
    assert!(work.publication_authorization.is_none());

    super::synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
    let run = runs.load(run_uid).unwrap();
    assert_eq!(run.phase, Phase::PrPending);
    assert_eq!(run.parent_task_progress.len(), 1);
    assert_eq!(
        run.parent_task_progress[0].pr_work_item_uid.as_deref(),
        Some(work_uid)
    );
    assert!(run.pr_url.is_none());
    super::assert_app_idle(app);

    pr_lifecycle::verify_persisted_pr_state_projection(
        app, lifecycle, runs, run_uid, task_uid, work_uid,
    );
}

#[path = "review_qa/pr_lifecycle.rs"]
mod pr_lifecycle;
#[path = "review_qa/qa_completion.rs"]
mod qa_completion;

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
