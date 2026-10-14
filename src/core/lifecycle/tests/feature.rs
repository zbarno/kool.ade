use super::*;

#[test]
fn plan_requires_resolved_questions_user_confirmation_and_independent_review() {
    let mut goal = feature();
    goal.apply(
        0,
        FeatureEvent::QuestionsRaised {
            questions: vec!["Which repository?".into()],
        },
    )
    .unwrap();
    assert!(
        goal.apply(
            1,
            FeatureEvent::PlanReady {
                plan_sha256: "b".repeat(64),
            }
        )
        .is_err()
    );
    goal.apply(1, FeatureEvent::AnswersResolved).unwrap();
    goal.apply(
        2,
        FeatureEvent::ReadinessRecorded {
            evidence_refs: vec!["acceptance-criteria/reviewed".into()],
        },
    )
    .unwrap();
    let plan_sha = goal.plan_sha256();
    goal.apply(
        3,
        FeatureEvent::PlanReady {
            plan_sha256: plan_sha.clone(),
        },
    )
    .unwrap();
    let feature_uid = goal.feature_uid.clone();
    let mut user = receipt(GateActor::User, GateDecision::Approved, &feature_uid);
    user.subject_sha256 = plan_sha.clone();
    goal.apply(4, FeatureEvent::UserPlanDecision(user)).unwrap();
    let mut review = receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &feature_uid,
    );
    set_prompt_role(
        &mut review,
        crate::harness::prompt_catalog::PromptRole::PlanReviewer,
    );
    review.subject_sha256 = plan_sha;
    goal.apply(5, FeatureEvent::PlanReview(review)).unwrap();
    assert_eq!(goal.phase, FeaturePhase::ReadyToDecompose);
    assert!(goal.apply(3, FeatureEvent::Cancel).is_err());
    goal.apply(6, FeatureEvent::Cancel).unwrap();
    assert!(goal.apply(7, FeatureEvent::Cancel).is_err());
}

#[test]
fn plan_hash_is_derived_from_exact_draft_and_edits_clear_readiness() {
    let mut goal = feature();
    let hash = goal.plan_sha256();
    assert!(
        goal.apply(
            0,
            FeatureEvent::PlanReady {
                plan_sha256: hash.clone(),
            },
        )
        .is_err()
    );
    goal.apply(
        0,
        FeatureEvent::ReadinessRecorded {
            evidence_refs: vec!["spec/acceptance-criteria".into()],
        },
    )
    .unwrap();
    assert!(
        goal.apply(
            1,
            FeatureEvent::PlanReady {
                plan_sha256: "f".repeat(64),
            },
        )
        .is_err()
    );
    goal.apply(
        1,
        FeatureEvent::DraftUpdated {
            draft: "Updated reviewed plan".into(),
            scope: vec!["workflow".into()],
            non_goals: vec!["deployment".into()],
        },
    )
    .unwrap();
    assert!(goal.readiness_evidence_refs.is_empty());
    assert_ne!(goal.plan_sha256(), hash);
}
