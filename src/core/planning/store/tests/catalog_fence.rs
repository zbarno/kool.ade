use super::*;
use crate::core::planning::{QuestionCategory, QuestionPriority, QuestionStatus};

fn question(uid: u128, feature_uid: &str) -> QuestionV1 {
    QuestionV1 {
        uid: uuid::Uuid::from_u128(uid).to_string(),
        feature_uid: feature_uid.into(),
        plan_revision: 1,
        decision_subject: "release ownership".into(),
        decision_domain: "ownership".into(),
        origin_requirement_ids: vec!["REQ-1".into()],
        category: QuestionCategory::Operations,
        priority: QuestionPriority::Blocking,
        question: "Who approves this release?".into(),
        reason: "The release boundary needs an owner.".into(),
        options: Vec::new(),
        recommended_option_id: None,
        no_answer_impact: "Release stays blocked.".into(),
        evidence_refs: Vec::new(),
        assumption_if_deferred: None,
        deferred: None,
        depends_on_question_ids: Vec::new(),
        status: QuestionStatus::Open,
        answer_text: None,
        evidence_resolution: None,
        selected_option: None,
        answered_by: None,
        answered_at_ms: None,
        deferred_by: None,
        deferred_at_ms: None,
        revision: 0,
    }
}

#[test]
// Production boundary: question insertion must invalidate a feature-level catalog
// snapshot used by lifecycle AnswersResolved. Record-only checks cannot detect an
// unseen UID; this test exercises the catalog revision transaction fence directly.
// Dependencies: temporary managed-local planning store; no services.
// Setup cost: low, using two synthetic blocking questions.
fn catalog_fence_rejects_a_question_added_after_the_snapshot() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};

    let root = std::env::temp_dir().join(format!("question-catalog-{}", uuid::Uuid::new_v4()));
    let planning = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let questions = QuestionStore::new(&planning);
    let feature_uid = uuid::Uuid::from_u128(44).to_string();
    questions.save(&question(45, &feature_uid), 0).unwrap();
    let snapshot_check = questions.feature_revision_check(&feature_uid).unwrap();

    questions.save(&question(46, &feature_uid), 0).unwrap();
    let result = planning.transaction_with_record_revisions(&[], &[snapshot_check]);

    assert!(matches!(
        result,
        Err(crate::artifacts::planning_store::StoreError::StaleRecordRevision { .. })
    ));
    let _ = std::fs::remove_dir_all(root);
}
