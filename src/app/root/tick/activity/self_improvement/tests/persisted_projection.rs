use super::*;

/// Production boundary: persisted opportunity decisions and lifecycle receipts flow through
/// the app synchronizer into the durable self-improvement run projection. A unit test of receipt
/// matching cannot prove the synchronizer reads persisted records in order or keeps the run from
/// Ready while approval is incomplete. Uses temporary local stores and Git only; no Docker/network.
/// Setup cost is one headless project fixture and a handful of local record writes.
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
    let feature_uid = run.feature_uid.clone();
    project.state.workflow.feature_branch_targets.insert(
        feature_uid.clone(),
        BranchTargets {
            source: "feat/fixture".into(),
            destination: "main".into(),
        },
    );

    let snapshot_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let hash = "a".repeat(64);
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
    synchronize_run(project, &lifecycle, &runs, &run.run_uid).unwrap();
    let projected = runs.load(&run.run_uid).unwrap();
    assert_eq!(projected.phase, Phase::Planning);
    assert!(projected.clone_uid.is_none());
    assert!(project.active_implementations.is_empty());

    let user = user_approval(&snapshot_uid, &hash);
    let feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::UserPlanDecision(user),
        )
        .unwrap();
    synchronize_run(project, &lifecycle, &runs, &run.run_uid).unwrap();
    let projected = runs.load(&run.run_uid).unwrap();
    assert_eq!(projected.phase, Phase::Planning);
    assert!(projected.clone_uid.is_none());
    assert!(project.active_implementations.is_empty());

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
    synchronize_run(project, &lifecycle, &runs, &run.run_uid).unwrap();

    let projected = runs.load(&run.run_uid).unwrap();
    assert_eq!(projected.phase, Phase::Ready);
    assert_eq!(projected.plan_approval_hash.as_deref(), Some(hash.as_str()));
    assert_eq!(
        projected.independent_plan_review_uid.as_deref(),
        Some(review_uid.as_str())
    );
    assert_eq!(projected.task_uids, vec![task_uid]);
    assert!(projected.clone_uid.is_none());
    assert!(project.active_implementations.is_empty());
    assert!(project.active_turn.is_none());
}

#[path = "persisted_projection/fixtures.rs"]
mod fixtures;
use fixtures::*;
