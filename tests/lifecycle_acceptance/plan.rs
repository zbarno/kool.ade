use super::support::*;
use koolade::{
    artifacts::planning_store::PlanningStore,
    core::{
        lifecycle::{
            FeatureEvent, FeatureGoal, FeaturePhase, GateActor, GateDecision, GateReceipt,
            LifecycleStore,
        },
        planning::{
            QuestionCategory, QuestionOption, QuestionPriority, QuestionStatus, QuestionStore,
            QuestionV1, reconcile_answer,
        },
    },
    harness::prompt_catalog::PromptRole,
};

pub(super) fn reviewed_feature_plan(
    store: &PlanningStore,
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
) -> FeatureGoal {
    // An asynchronous unanswered card blocks readiness; replaying its answer
    // after restart is represented by applying the answer event once.
    let mut feature = lifecycle.load_feature(feature_uid).unwrap();
    feature = lifecycle
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::QuestionsRaised {
                questions: vec!["Should Escape close the dialog while a request is active?".into()],
            },
        )
        .unwrap();
    assert_eq!(feature.phase, FeaturePhase::AwaitingAnswers);
    let card = QuestionV1 {
        uid: id(),
        feature_uid: feature_uid.to_owned(),
        plan_revision: feature.revision,
        decision_subject: "Active search cancellation".into(),
        decision_domain: "UX and failure behavior".into(),
        origin_requirement_ids: vec!["REQ-001".into()],
        category: QuestionCategory::FailureMode,
        priority: QuestionPriority::Blocking,
        question: "Should Escape cancel an active search request?".into(),
        reason: "The answer changes interaction and request lifecycle behavior".into(),
        options: vec![QuestionOption {
            id: "cancel".into(),
            label: "Close and cancel".into(),
            tradeoffs: "Prevents stale results from appearing".into(),
        }],
        recommended_option_id: Some("cancel".into()),
        no_answer_impact: "Plan readiness remains blocked".into(),
        evidence_refs: vec!["context:search-dialog".into()],
        assumption_if_deferred: None,
        deferred: None,
        depends_on_question_ids: Vec::new(),
        status: QuestionStatus::Open,
        answer_text: None,
        selected_option: None,
        answered_by: None,
        answered_at_ms: None,
        revision: 0,
    };
    QuestionStore::new(store).save(&card, 0).unwrap();
    let reopened_store = PlanningStore::new(store.project_id, store.root.clone(), store.mode);
    let reopened = LifecycleStore::new(&reopened_store);
    let questions = QuestionStore::new(&reopened_store);
    let (mut card, card_record_revision) = questions.load(&card.uid).unwrap();
    let card_revision = card.revision;
    reconcile_answer(
        &mut card,
        card_revision,
        feature.revision,
        Some("Escape closes the dialog and cancels its request"),
        Some("cancel"),
        "user:fixture",
        10,
    )
    .unwrap();
    questions.save(&card, card_record_revision).unwrap();
    let (saved_card, saved_revision) = questions.load(&card.uid).unwrap();
    assert_eq!(saved_card.status, QuestionStatus::Answered);
    let mut replay = saved_card.clone();
    assert!(
        reconcile_answer(
            &mut replay,
            saved_card.revision,
            feature.revision,
            Some("Escape closes the dialog and cancels its request"),
            Some("cancel"),
            "user:fixture",
            11,
        )
        .is_err(),
        "replayed answer cannot create another decision"
    );
    assert_eq!(questions.load(&card.uid).unwrap().1, saved_revision);
    feature = reopened
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::AnswersResolved,
        )
        .unwrap();
    feature = reopened
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::DraftUpdated {
                draft: "Escape closes the dialog and cancels its in-flight request".into(),
                scope: vec!["Search dialog".into(), "Escape cancellation".into()],
                non_goals: vec!["Deployment".into()],
            },
        )
        .unwrap();
    feature = reopened
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::ReadinessRecorded {
                evidence_refs: vec![
                    "decision:escape-cancels-request".into(),
                    "acceptance:keyboard-dismissal".into(),
                ],
            },
        )
        .unwrap();

    let plan_hash = feature.plan_sha256();
    feature = reopened
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::PlanReady {
                plan_sha256: plan_hash.clone(),
            },
        )
        .unwrap();
    let mut confirmation = GateReceipt {
        receipt_uid: id(),
        subject_uid: feature_uid.to_owned(),
        subject_sha256: plan_hash.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 2,
        evidence_refs: vec!["user-action:confirm-plan".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    feature = reopened
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::UserPlanDecision(confirmation.clone()),
        )
        .unwrap();
    // Independent reviewer catches a missing failure case. Material repair
    // changes the plan hash and requires a new user confirmation and review.
    let rejected = role_receipt(
        PromptRole::PlanReviewer,
        GateDecision::ChangesRequested,
        feature_uid,
        &plan_hash,
    );
    feature = reopened
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::PlanReview(rejected),
        )
        .unwrap();
    assert_eq!(feature.phase, FeaturePhase::PlanRework);
    feature = reopened.apply_feature_event(feature_uid, feature.revision, &id(), FeatureEvent::DraftUpdated {
        draft: "Escape cancels active requests, restores focus, and reports cancellation errors".into(),
        scope: vec!["Search dialog".into(), "Escape cancellation".into(), "Focus restoration".into()],
        non_goals: vec!["Deployment".into()],
    }).unwrap();
    feature = reopened
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::ReadinessRecorded {
                evidence_refs: vec![
                    "decision:escape-cancels-request".into(),
                    "acceptance:focus-restoration".into(),
                ],
            },
        )
        .unwrap();
    let approved_hash = feature.plan_sha256();
    feature = reopened
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::PlanReady {
                plan_sha256: approved_hash.clone(),
            },
        )
        .unwrap();
    confirmation.receipt_uid = id();
    confirmation.subject_sha256 = approved_hash.clone();
    feature = reopened
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::UserPlanDecision(confirmation),
        )
        .unwrap();
    let passed = role_receipt(
        PromptRole::PlanReviewer,
        GateDecision::Approved,
        feature_uid,
        &approved_hash,
    );
    feature = reopened
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::PlanReview(passed),
        )
        .unwrap();
    feature = reopened
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::TaskGraphCreated {
                plan_sha256: approved_hash.clone(),
            },
        )
        .unwrap();
    assert_eq!(feature.phase, FeaturePhase::Executing);
    feature
}
