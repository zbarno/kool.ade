use super::*;
use crate::core::lifecycle::{
    FeaturePhase, SpecDeltaClassificationV1, SpecDeltaOperatorDecisionV1, SpecDeltaRecordV1,
};

fn fixture() -> (
    std::path::PathBuf,
    LifecycleStore<'static>,
    FeatureGoal,
    SpecRevisionDraftV1,
) {
    let root = std::env::temp_dir().join(format!("feature-target-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .unwrap();
    crate::artifacts::migration::bootstrap_product(&root, "Target Adoption Fixture").unwrap();
    let store = Box::leak(Box::new(
        crate::artifacts::planning_store::PlanningStore::new(
            uuid::Uuid::new_v4(),
            root.join(".koolade-packet"),
            crate::artifacts::planning_store::StoreMode::ManagedLocal,
        ),
    ));
    let feature_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let origin = uuid::Uuid::new_v4().hyphenated().to_string();
    let feature = FeatureGoal {
        schema_version: 1,
        feature_uid: feature_uid.clone(),
        original_intent: "target adoption".into(),
        draft: String::new(),
        scope: Vec::new(),
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
    let lifecycle = LifecycleStore::new(store);
    lifecycle.create_feature(&feature, &origin).unwrap();
    let content_hash = crate::core::lifecycle::product_spec::current_product_hash(store).unwrap();
    let receipt_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let draft = SpecRevisionDraftV1 {
        no_change_reason: None,
        content_hash: content_hash.clone(),
        source_revision: 1,
        produced_by_run_id: None,
        origin_events: vec![origin.clone()],
        decision_refs: vec![receipt_uid.clone()],
        delta_approval_refs: Vec::new(),
        approved_target_hash: Some(content_hash.clone()),
        implementation_head_sha: None,
        reconciliation_status: crate::core::lifecycle::SpecReconciliationStatusV1::Synchronized,
        outcome_reason: None,
        deltas: vec![SpecDeltaRecordV1 {
            requirement_id: "REQ-1".into(),
            event_id: origin,
            old_target: Some("REQ-1 old".into()),
            proposed_target: Some("REQ-1 new".into()),
            observed_current: None,
            classification: SpecDeltaClassificationV1::UserDecision,
            evidence_refs: vec!["fixture".into()],
            decision_refs: Vec::new(),
            test_refs: Vec::new(),
            affected_task_ids: Vec::new(),
            needs_user_reapproval: false,
            followup_ids: Vec::new(),
            operator_decision: Some(SpecDeltaOperatorDecisionV1 {
                receipt_uid,
                decision: SpecDeltaOperatorChoiceV1::Adopted,
                actor: crate::core::lifecycle::GateActor::User,
                recorded_at_ms: 1,
                resulting_spec_sha256: Some(content_hash),
            }),
        }],
    };
    (root, lifecycle, feature, draft)
}

#[test]
fn fabricated_operator_fields_cannot_authorize_feature_target_adoption() {
    let (root, lifecycle, feature, draft) = fixture();
    assert!(
        validate_approved_target(
            &lifecycle,
            &feature.feature_uid,
            0,
            &feature,
            None,
            &draft,
            None,
        )
        .is_err()
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mismatched_feature_adoption_result_cannot_advance_approved_target() {
    let (root, lifecycle, feature, mut draft) = fixture();
    draft.deltas[0]
        .operator_decision
        .as_mut()
        .unwrap()
        .resulting_spec_sha256 = Some("f".repeat(64));
    assert!(
        validate_approved_target(
            &lifecycle,
            &feature.feature_uid,
            0,
            &feature,
            None,
            &draft,
            None,
        )
        .is_err()
    );
    let _ = std::fs::remove_dir_all(root);
}
