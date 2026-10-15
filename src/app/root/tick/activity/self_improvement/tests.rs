use super::{reviewed_plan_receipts, transition_uid};
use crate::core::lifecycle::{
    FeatureEvent, FeatureGoal, GateActor, GateDecision, GateReceipt, LifecycleActor,
    LifecycleEventKind, LifecycleEventRecord,
};
use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render};

#[test]
fn lifecycle_projection_event_ids_are_stable_across_restarts_and_retries() {
    let run_uid = "00000000-0000-4000-8000-000000000001";
    let evidence = "code-review:00000000-0000-4000-8000-000000000002";
    let first = transition_uid(run_uid, evidence).unwrap();
    let after_restart = transition_uid(run_uid, evidence).unwrap();
    let different_receipt =
        transition_uid(run_uid, "code-review:00000000-0000-4000-8000-000000000003").unwrap();
    let other_run = transition_uid("00000000-0000-4000-8000-000000000004", evidence).unwrap();

    assert_eq!(first, after_restart);
    assert_ne!(first, different_receipt);
    assert_ne!(first, other_run);
}

#[test]
fn plan_projection_requires_the_exact_user_and_independent_review_receipts() {
    let feature_uid = "00000000-0000-4000-8000-000000000001".to_owned();
    let snapshot_uid = "00000000-0000-4000-8000-000000000002".to_owned();
    let hash = "a".repeat(64);
    let feature = FeatureGoal {
        schema_version: 1,
        feature_uid: feature_uid.clone(),
        original_intent: "reviewed plan fixture".into(),
        draft: "reviewed plan fixture".into(),
        scope: vec![],
        non_goals: vec![],
        revision: 5,
        phase: crate::core::lifecycle::FeaturePhase::ReadyToDecompose,
        approved_plan_sha256: Some(hash.clone()),
        plan_snapshot_uid: Some(snapshot_uid.clone()),
        plan_snapshot_sha256: Some(hash.clone()),
        unresolved_questions: vec![],
        decision_refs: vec!["00000000-0000-4000-8000-000000000003".into()],
        review_refs: vec!["00000000-0000-4000-8000-000000000004".into()],
        readiness_evidence_refs: vec!["fixture/readiness".into()],
        task_uids: vec![],
    };
    let user = GateReceipt {
        receipt_uid: feature.decision_refs[0].clone(),
        subject_uid: snapshot_uid.clone(),
        subject_sha256: hash.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture/user-confirmation".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let manifest = render(&RoleInputV1 {
        role: PromptRole::PlanReviewer,
        workspace_uid: "00000000-0000-4000-8000-000000000005".into(),
        feature_uid: feature_uid.clone(),
        task_uid: None,
        original_goal: feature.original_intent.clone(),
        approved_spec_ref: Some(snapshot_uid.clone()),
        approved_spec_hash: Some(hash.clone()),
        decision_refs: vec![],
        evidence_manifest: vec![],
        execution_policy_ref: "self-improvement-test/v1".into(),
        output_schema_version: PromptRole::PlanReviewer.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: "fixture-revision".into(),
            subject_hash: hash.clone(),
            author_run_id: "00000000-0000-4000-8000-000000000006".into(),
            reviewer_run_id: "00000000-0000-4000-8000-000000000007".into(),
            author_session_id: Some("00000000-0000-4000-8000-000000000008".into()),
            reviewer_session_id: Some("00000000-0000-4000-8000-000000000009".into()),
            requirements_matrix: vec!["fixture requirement".into()],
            explicit_exclusions: vec![],
        },
    })
    .unwrap()
    .manifest;
    let review = GateReceipt {
        receipt_uid: feature.review_refs[0].clone(),
        subject_uid: snapshot_uid,
        subject_sha256: hash.clone(),
        actor: GateActor::IndependentReviewer,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture/independent-review".into()],
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some("00000000-0000-4000-8000-000000000007".into()),
        author_run_id: Some("00000000-0000-4000-8000-000000000006".into()),
        reviewer_session_id: Some("00000000-0000-4000-8000-000000000009".into()),
        author_session_id: Some("00000000-0000-4000-8000-000000000008".into()),
    };
    let records = vec![
        event(&feature_uid, FeatureEvent::UserPlanDecision(user)),
        event(&feature_uid, FeatureEvent::PlanReview(review)),
    ];

    let (user, review) = reviewed_plan_receipts(&feature, &records, &hash).unwrap();
    assert_eq!(user.receipt_uid, feature.decision_refs[0]);
    assert_eq!(review.receipt_uid, feature.review_refs[0]);

    let mut stale_feature = feature.clone();
    stale_feature.review_refs[0] = "00000000-0000-4000-8000-000000000010".into();
    assert!(reviewed_plan_receipts(&stale_feature, &records, &hash).is_none());
    assert!(reviewed_plan_receipts(&feature, &records, &"b".repeat(64)).is_none());
}

fn event(feature_uid: &str, event: FeatureEvent) -> LifecycleEventRecord {
    let actor = match &event {
        FeatureEvent::UserPlanDecision(_) => LifecycleActor::User,
        FeatureEvent::PlanReview(_) => LifecycleActor::IndependentReviewer,
        _ => LifecycleActor::Manager,
    };
    LifecycleEventRecord {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        schema_version: 1,
        created_at_ms: 1,
        updated_at_ms: 1,
        revision: 1,
        feature_uid: feature_uid.to_owned(),
        task_uid: None,
        expected_revision: 0,
        actor,
        event: LifecycleEventKind::Feature(event),
        feature_revision_after: None,
        task_revision_after: None,
        initial_feature: None,
        initial_task: None,
        parent_task_after: None,
        pr_preparation_after: None,
    }
}
