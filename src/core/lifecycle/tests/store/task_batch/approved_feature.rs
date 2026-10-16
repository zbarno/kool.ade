use super::super::super::{feature, receipt, set_prompt_role};
use crate::core::lifecycle::{FeatureEvent, FeaturePhase, GateActor, GateDecision, LifecycleStore};

pub(super) fn approved_feature(lifecycle: &LifecycleStore<'_>) -> (String, String) {
    let mut feature = feature();
    let raw = "# CHG-810: Task batch fixture\n\n## Intent\n\nCreate lifecycle tasks.\n\n## Current Behavior\n\nNo task batch exists.\n\n## Desired Behavior\n\nApproved task batches are persisted.\n\n## Scope\n\nTask batch persistence.\n\n## Requirements\n\n- TASK-R1: Persist the approved task batch.\n\n## Decisions and Assumptions\n\n- **A1:** Use the canonical lifecycle store.\n\n## Acceptance Criteria\n\n1. Tasks resolve to the approved requirement.\n";
    let markdown = crate::domain::ArtifactIdentity::preserve_markdown(
        raw,
        None,
        "CHG-810",
        "Task batch fixture",
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .unwrap();
    let markdown = crate::domain::ChangeMetadata::write_markdown(
        &markdown,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    let snapshot = crate::core::lifecycle::PlanSnapshotV1::from_feature(
        "CHG-810",
        &identity,
        &markdown,
        &crate::core::workflow::feature_contract(&markdown),
    )
    .unwrap();
    let store = lifecycle.store;
    let (snapshot_path, snapshot_bytes, snapshot_check) =
        crate::core::lifecycle::plan_snapshot::record_change(store, &snapshot)
            .unwrap()
            .unwrap();
    store
        .transaction_with_record_revisions(&[(snapshot_path, snapshot_bytes)], &[snapshot_check])
        .unwrap();
    let feature_uid = identity.uid.clone();
    feature.feature_uid = feature_uid.clone();
    feature.plan_snapshot_uid = Some(snapshot.uid.clone());
    feature.plan_snapshot_sha256 = Some(snapshot.content_hash.clone());
    let plan_sha = snapshot.content_hash.clone();
    lifecycle
        .create_feature(&feature, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    let mut feature = lifecycle.load_feature(&feature_uid).unwrap();
    feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::ReadinessRecorded {
                evidence_refs: vec!["readiness/current-spec".into()],
            },
        )
        .unwrap();
    feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::PlanReady {
                plan_sha256: plan_sha.clone(),
            },
        )
        .unwrap();
    let mut user = receipt(GateActor::User, GateDecision::Approved, &snapshot.uid);
    user.subject_sha256 = plan_sha.clone();
    feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::UserPlanDecision(user),
        )
        .unwrap();
    let mut review = receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &snapshot.uid,
    );
    set_prompt_role(
        &mut review,
        crate::harness::prompt_catalog::PromptRole::PlanReviewer,
    );
    review.subject_sha256 = plan_sha.clone();
    let feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::PlanReview(review),
        )
        .unwrap();
    assert_eq!(feature.phase, FeaturePhase::ReadyToDecompose);
    (feature_uid, plan_sha)
}
