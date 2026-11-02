use crate::core::lifecycle::{TaskEvent, TaskPhase};
use crate::core::self_improvement::{SelfImprovementPhaseV1 as Phase, SelfImprovementStore};
use crate::harness::prompt_catalog::PromptRole;

use super::{apply_task, reviewer_receipt};

pub(super) fn verify_post_qa_spec_repair(
    app: &mut crate::app::root::KooladeApp,
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    runs: &SelfImprovementStore<'_>,
    run_uid: &str,
    task_uid: &str,
    old_work_uid: &str,
) -> String {
    let old_run = runs.load(run_uid).unwrap();
    assert_eq!(old_run.phase, Phase::PrPending);
    assert_eq!(old_run.code_review_uids.len(), 1);
    assert_eq!(old_run.runtime_qa_uids.len(), 1);
    assert!(old_run.spec_receipt_uid.is_some());
    let old_head_sha = "d".repeat(40);
    assert_eq!(
        old_run.implementation_head_sha.as_deref(),
        Some(old_head_sha.as_str())
    );
    let task = lifecycle.load_task(task_uid).unwrap();
    assert_eq!(task.phase, TaskPhase::Done);
    assert!(task.code_review.is_some());
    assert!(task.runtime_qa.is_some());
    let old_spec_receipt_uid = task
        .spec_reconciliation
        .as_ref()
        .expect("old implementation has a spec receipt")
        .receipt_uid
        .clone();

    // Destination integration changes the reviewed head after all old gates
    // passed. This application-owned event clears the task receipts and starts
    // a bounded re-review; the old PR item remains unauthorized and unused.
    let repaired_sha256 = task.implementation_sha256.clone().unwrap();
    let repaired_head = "9".repeat(40);
    lifecycle
        .apply_task_event(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            TaskEvent::PublicationHeadChanged {
                code_sha256: repaired_sha256.clone(),
                head_sha: repaired_head.clone(),
            },
        )
        .unwrap();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    project
        .implementation_states
        .values_mut()
        .find(|state| state.task_uid.as_deref() == Some(task_uid))
        .expect("validated task clone implementation")
        .verified_head = Some(repaired_head.clone());

    super::super::synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
    let run = runs.load(run_uid).unwrap();
    assert_eq!(run.phase, Phase::Reviewing);
    assert_eq!(
        run.implementation_subject_sha256,
        old_run.implementation_subject_sha256
    );
    assert_ne!(run.implementation_head_sha, old_run.implementation_head_sha);
    assert_eq!(
        run.implementation_subject_sha256.as_deref(),
        Some(repaired_sha256.as_str())
    );
    assert_eq!(
        run.implementation_head_sha.as_deref(),
        Some(repaired_head.as_str())
    );
    assert!(run.code_review_uids.is_empty());
    assert!(run.runtime_qa_uids.is_empty());
    assert!(run.spec_receipt_uid.is_none());
    assert_eq!(run.parent_task_progress[0].phase, TaskPhase::ReReview);

    let mut task = lifecycle.load_task(task_uid).unwrap();
    assert!(task.code_review.is_none());
    assert!(task.runtime_qa.is_none());
    assert!(task.spec_reconciliation.is_none());
    let review = reviewer_receipt(
        PromptRole::CodeReviewer,
        &task,
        &repaired_sha256,
        &repaired_head,
        uuid::Uuid::new_v4().hyphenated().to_string(),
        uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let author_run_id = review.author_run_id.clone().unwrap();
    let author_session_id = review.author_session_id.clone().unwrap();
    task = apply_task(
        lifecycle,
        &task,
        TaskEvent::CodeReview {
            receipt: review,
            reviewed_head_sha: repaired_head.clone(),
            review_subject_sha256: "e".repeat(64),
        },
    );
    super::super::synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
    assert_eq!(runs.load(run_uid).unwrap().phase, Phase::Qa);

    let request = crate::runtime_qa::QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        parent_task_uid: task.task_uid.clone(),
        target_id: Some("fixture-target".into()),
        planning_item_uid: Some(task.task_uid.clone()),
        feature_uid: task.feature_uid.clone(),
        code_head_sha: repaired_head.clone(),
        build_artifact_sha256: "a".repeat(64),
        complete_diff_hash: repaired_sha256.clone(),
        review_subject_sha256: "e".repeat(64),
        approved_spec_hash: task.approved_plan_sha256.clone(),
        requirement_ids: vec!["REQ-001".into()],
        acceptance_criteria: vec!["The repaired behavior is ready.".into()],
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
        author_run_id,
        author_session_id,
    };
    task = apply_task(
        lifecycle,
        &task,
        TaskEvent::RuntimeQaRequested(request.clone()),
    );
    let qa_root = tempfile::tempdir().unwrap();
    super::qa_completion::run_and_record(lifecycle, &task, &request, qa_root.path());
    super::super::synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
    assert_eq!(runs.load(run_uid).unwrap().phase, Phase::SpecSync);

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
        "repaired spec reconciliation remained blocked: {:?}",
        project.activity.pending
    );
    let refreshed_spec = completed
        .spec_reconciliation
        .as_ref()
        .expect("repaired implementation has a spec receipt");
    assert_ne!(refreshed_spec.receipt_uid, old_spec_receipt_uid);
    assert_eq!(refreshed_spec.subject_sha256, repaired_sha256);
    let work_uid = completed
        .pr_work_item_uid
        .as_deref()
        .expect("fresh spec reconciliation creates PR preparation");
    assert_eq!(work_uid, old_work_uid);
    super::super::synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
    assert_eq!(runs.load(run_uid).unwrap().phase, Phase::PrPending);
    work_uid.to_owned()
}
