use super::*;

/// Production boundary: persisted opportunity, approval, clone, code-review, and runtime-QA
/// records flow through the app synchronizer into the durable self-improvement run projection,
/// including the first synchronization after reopening the project.
/// A unit test of receipt matching cannot prove the synchronizer reads the persisted gates in
/// order, restores project state after restart, or keeps the run from advancing before each
/// receipt exists. Uses temporary local stores and Git only; no Docker or network. Setup cost is
/// one headless project fixture and local writes.
#[test]
fn synchronize_requires_persisted_user_and_independent_plan_approval_before_ready() {
    use crate::core::{
        lifecycle::{FeatureEvent, FeatureGoal, LifecycleStore, TaskPhase, TaskRecord},
        opportunity::{OpportunityDecisionKindV1, OpportunityStore},
        self_improvement::{SelfImprovementPhaseV1 as Phase, SelfImprovementStore},
        workflow::BranchTargets,
    };

    let mut app = crate::app::root::board_tests::fixture();
    let uid = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        test_product_proposal(project)
    };
    let context = egui::Context::default();
    crate::app::root::board_tests::click_text(&mut app, &context, "Improvement proposals (1)");
    crate::app::root::board_tests::click_text(&mut app, &context, "Improve planning reliability");
    crate::app::root::board_tests::click_text(&mut app, &context, "Pursue");
    let (planning, run_uid, feature_uid, feature_id) = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should remain connected")
        };
        let planning = project.state.planning_store.clone();
        let opportunities = OpportunityStore::new(&planning);
        assert_eq!(
            opportunities
                .decision_for_opportunity(&uid)
                .unwrap()
                .unwrap()
                .decision,
            OpportunityDecisionKindV1::Pursue
        );
        let run_uid = SelfImprovementStore::run_uid_for_decision(
            &opportunities
                .decision_for_opportunity(&uid)
                .unwrap()
                .unwrap()
                .uid,
        )
        .unwrap();
        let runs = SelfImprovementStore::new(&planning);
        let run = runs.load(&run_uid).unwrap();
        assert_eq!(run.phase, Phase::Planning);
        let feature_id = project
            .state
            .active_feature
            .as_ref()
            .map(|(id, _)| id.clone())
            .expect("Pursue creates its draft product feature");
        project.state.workflow.feature_branch_targets.insert(
            feature_id.clone(),
            BranchTargets {
                source: "feat/fixture".into(),
                destination: "main".into(),
            },
        );
        crate::artifacts::task_docs::save_workflow(
            &project.state.planning_store,
            &project.state.workflow,
        )
        .unwrap();
        (planning, run_uid, run.feature_uid, feature_id)
    };
    let runs = SelfImprovementStore::new(&planning);

    let identity = crate::domain::ArtifactIdentity {
        uid: feature_uid.clone(),
        display_id: feature_id.clone(),
        title: "Persisted approval fixture".into(),
        parent_uid: None,
    };
    let markdown = "# Persisted approval fixture\n\n## Intent\n\nExercise persisted self-improvement approval.\n\n## Current Behavior\n\nThe fixture is not yet approved.\n\n## Desired Behavior\n\nThe approved capability remains bound to its stable ID.\n\n## Capability ID\n\nself-improvement-lifecycle\n\n## Scope\n\n- planning approval projection\n\n## Requirements\n\n- Bind the approved capability ID.\n\n## Acceptance Criteria\n\n- AC-001: The capability ID is persisted in the approved snapshot.\n";
    let snapshot = crate::core::lifecycle::PlanSnapshotV1::from_feature(
        &feature_id,
        &identity,
        markdown,
        "self-improvement test approval contract",
    )
    .unwrap();
    let (snapshot_path, snapshot_bytes, snapshot_check) =
        crate::core::lifecycle::plan_snapshot::record_change(&planning, &snapshot)
            .unwrap()
            .unwrap();
    planning
        .transaction_with_record_revisions(&[(snapshot_path, snapshot_bytes)], &[snapshot_check])
        .unwrap();
    let snapshot_uid = snapshot.uid.clone();
    let hash = snapshot.content_hash.clone();
    let lifecycle = LifecycleStore::new(&planning);
    lifecycle
        .create_feature(
            &FeatureGoal {
                schema_version: 1,
                feature_uid: feature_uid.clone(),
                original_intent: "Persisted approval fixture".into(),
                draft: "A fully reviewed plan".into(),
                scope: vec!["planning".into()],
                non_goals: vec!["deployment".into()],
                revision: 0,
                phase: crate::core::lifecycle::FeaturePhase::Exploring,
                approved_plan_sha256: None,
                plan_snapshot_uid: Some(snapshot_uid.clone()),
                plan_snapshot_sha256: Some(hash.clone()),
                unresolved_questions: vec![],
                decision_refs: vec![],
                review_refs: vec![],
                readiness_evidence_refs: vec![],
                task_uids: vec![],
            },
            &uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
    let feature = lifecycle.load_feature(&feature_uid).unwrap();
    let feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::QuestionsRaised {
                questions: vec!["Who owns the approval boundary?".into()],
            },
        )
        .unwrap();
    let question_uid = question_flow::answer_product_question(&mut app, &feature_uid);
    let question_store = crate::core::planning::QuestionStore::new(&planning);
    let (answered_question, _) = question_store.load(&question_uid).unwrap();
    assert_eq!(
        answered_question.status,
        crate::core::planning::QuestionStatus::Answered
    );
    assert_eq!(
        answered_question.answer_text.as_deref(),
        Some("The workspace operator owns approval.")
    );

    question_flow::verify_answer_resolution_gate(
        &lifecycle,
        &planning,
        &feature_uid,
        feature.revision,
        &answered_question,
    );
    let feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::AnswersResolved,
        )
        .unwrap();
    let feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::ReadinessRecorded {
                evidence_refs: vec!["fixture/readiness".into()],
            },
        )
        .unwrap();
    let feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::PlanReady {
                plan_sha256: hash.clone(),
            },
        )
        .unwrap();
    let mut forged_user_approval = user_approval(&snapshot_uid, &hash);
    forged_user_approval.author_run_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    assert!(
        lifecycle
            .apply_feature_event(
                &feature_uid,
                feature.revision,
                &uuid::Uuid::new_v4().to_string(),
                FeatureEvent::UserPlanDecision(forged_user_approval),
            )
            .is_err(),
        "agent-attributed data cannot be persisted as a user plan approval"
    );
    synchronize_app_run(&mut app, &lifecycle, &runs, &run_uid).unwrap();
    let projected = runs.load(&run_uid).unwrap();
    assert_eq!(projected.phase, Phase::Planning);
    assert!(projected.clone_uid.is_none());
    assert_app_idle(&mut app);

    let user = user_approval(&snapshot_uid, &hash);
    let feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::UserPlanDecision(user),
        )
        .unwrap();
    synchronize_app_run(&mut app, &lifecycle, &runs, &run_uid).unwrap();
    let projected = runs.load(&run_uid).unwrap();
    assert_eq!(projected.phase, Phase::Planning);
    assert!(projected.clone_uid.is_none());
    assert_app_idle(&mut app);

    let mut forged_review = independent_plan_review(&feature_uid, &snapshot_uid, &hash);
    forged_review.reviewer_run_id = forged_review.author_run_id.clone();
    forged_review.reviewer_session_id = forged_review.author_session_id.clone();
    assert!(
        lifecycle
            .apply_feature_event(
                &feature_uid,
                feature.revision,
                &uuid::Uuid::new_v4().to_string(),
                FeatureEvent::PlanReview(forged_review),
            )
            .is_err(),
        "a reviewer receipt reusing the plan author's run and session is not independent"
    );
    synchronize_app_run(&mut app, &lifecycle, &runs, &run_uid).unwrap();
    let projected = runs.load(&run_uid).unwrap();
    assert_eq!(projected.phase, Phase::Planning);
    assert!(projected.clone_uid.is_none());
    assert_app_idle(&mut app);

    let review = independent_plan_review(&feature_uid, &snapshot_uid, &hash);
    let review_uid = review.receipt_uid.clone();
    let feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::PlanReview(review),
        )
        .unwrap();
    lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::TaskGraphCreated {
                plan_sha256: hash.clone(),
            },
        )
        .unwrap();
    let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    lifecycle
        .create_task(
            &TaskRecord {
                schema_version: 1,
                task_uid: task_uid.clone(),
                feature_uid: feature_uid.clone(),
                planning_item_uid: None,
                task_spec_sha256: None,
                parent_task_uid: None,
                subtask_uids: vec![],
                dependency_task_uids: vec![],
                approved_requirement_ids: vec![],
                phase: TaskPhase::Ready,
                revision: 0,
                approved_plan_sha256: hash.clone(),
                implementation_sha256: None,
                reviewed_code_head_sha: None,
                reviewed_code_subject_sha256: None,
                code_review: None,
                review_history: vec![],
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
            &uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
    let _ = feature;
    let manifest = crate::core::project_repos::ProjectManifest::load(&planning).unwrap();
    assert_eq!(
        manifest
            .repositories
            .iter()
            .find(|repository| repository.id == "root")
            .unwrap()
            .remote,
        "https://github.com/zbarno/kool.ade"
    );
    assert_eq!(
        crate::core::project_repos::stable_uid(
            &planning.project_id.hyphenated().to_string(),
            "repository",
            "root",
        ),
        runs.load(&run_uid).unwrap().target_repository_uid
    );
    let repo_root = match &app.screen {
        crate::app::root::Screen::Connected(project) => project.state.repo_root.clone(),
        _ => panic!("fixture should remain connected"),
    };
    let mut reopened = crate::app::session::test_support::project_from(&repo_root);
    reopened.state =
        crate::core::state::PlannerState::load_with_store(&repo_root, &planning).unwrap();
    app.screen = crate::app::root::Screen::Connected(Box::new(reopened));

    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("reopened fixture should remain connected")
    };
    assert!(project.active_turn.is_none());
    let reopened_opportunities = OpportunityStore::new(&project.state.planning_store);
    assert!(
        reopened_opportunities
            .list()
            .unwrap()
            .iter()
            .any(|proposal| proposal.opportunity_uid == uid)
    );
    let reopened_proposal = reopened_opportunities.load(&uid).unwrap().0;
    assert_eq!(
        reopened_proposal.target.kind,
        crate::core::opportunity::OpportunityTargetKindV1::KooladeProduct
    );
    assert!(
        reopened_proposal
            .related_feature_uids
            .contains(&feature_uid)
    );
    let reopened_decision = reopened_opportunities
        .decision_for_opportunity(&uid)
        .unwrap()
        .unwrap();
    assert_eq!(
        reopened_decision.decision,
        OpportunityDecisionKindV1::Pursue
    );
    assert!(
        project
            .state
            .planning_store
            .list_record_paths("state/lifecycle_features")
            .unwrap()
            .contains(&format!("state/lifecycle_features/{feature_uid}.json"))
    );
    let reopened_feature = LifecycleStore::new(&project.state.planning_store)
        .load_feature(&feature_uid)
        .unwrap();
    assert_eq!(
        reopened_feature.approved_plan_sha256.as_deref(),
        Some(hash.as_str())
    );
    let reopened_run = SelfImprovementStore::new(&project.state.planning_store)
        .load(&run_uid)
        .unwrap();
    assert_eq!(reopened_run.feature_uid, feature_uid);
    assert_eq!(reopened_run.approved_decision_uid, reopened_decision.uid);
    assert_eq!(
        SelfImprovementStore::run_uid_for_decision(&reopened_decision.uid).unwrap(),
        run_uid
    );
    assert!(
        reopened_proposal
            .related_feature_uids
            .contains(&reopened_run.feature_uid)
    );
    app.advance_activity();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("activity tick should keep the reopened fixture connected")
    };
    let reopened_runs = SelfImprovementStore::new(&project.state.planning_store);
    let projected = reopened_runs.load(&run_uid).unwrap();
    assert_eq!(
        projected.phase,
        Phase::Ready,
        "startup activity did not reconcile the run: {:?}",
        project.activity.pending
    );
    assert_eq!(projected.plan_approval_hash.as_deref(), Some(hash.as_str()));
    assert_eq!(
        projected.capability_id.as_deref(),
        Some("self-improvement-lifecycle")
    );
    assert_eq!(
        projected.independent_plan_review_uid.as_deref(),
        Some(review_uid.as_str())
    );
    assert_eq!(projected.task_uids, vec![task_uid.clone()]);
    assert!(projected.clone_uid.is_none());
    assert_app_idle(&mut app);

    clone_dispatch::verify_clone_dispatch(&mut app, &lifecycle, &runs, &run_uid, &task_uid);
    review_qa::verify_reviewer_and_qa_projection(&mut app, &lifecycle, &runs, &run_uid, &task_uid);
}

#[path = "persisted_projection/clone_dispatch.rs"]
mod clone_dispatch;
#[path = "persisted_projection/fixtures.rs"]
mod fixtures;
#[path = "persisted_projection/question_flow.rs"]
mod question_flow;
#[path = "persisted_projection/review_qa.rs"]
mod review_qa;
use fixtures::*;
