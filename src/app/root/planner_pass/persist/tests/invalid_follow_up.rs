use super::*;

#[test]
// Production boundary: invalid planner output must not retire durable cards before
// its replacement questions and readiness are validated. A unit test cannot prove
// persisted records remain unchanged across the planner reconciliation path.
// Dependencies: temporary managed-local planning store; no services.
// Setup cost: low, with synthetic feature artifacts.
fn invalid_follow_up_output_does_not_partially_supersede_questions() {
    use crate::{
        artifacts::planning_store::{PlanningStore, StoreMode},
        core::planning::QuestionStore,
    };
    let root = std::env::temp_dir().join(format!("planner-atomic-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (feature_uid, spec_revision) = install_feature(&store);
    persist(
        &store,
        &feature_uid,
        1,
        &spec_revision,
        planner_output(&feature_uid),
    )
    .unwrap();
    let questions = QuestionStore::new(&store);
    let original = questions.list().unwrap().pop().unwrap();
    let readiness_before = questions.load_readiness(&feature_uid).unwrap();

    let mut invalid = planner_output(&feature_uid);
    invalid.proposed_questions[0].origin_requirement_ids = vec!["not-a-requirement".into()];
    assert!(persist(&store, &feature_uid, 2, &spec_revision, invalid).is_err());

    let (saved, _) = questions.load(&original.uid).unwrap();
    assert_eq!(saved, original);
    assert_eq!(
        questions.load_readiness(&feature_uid).unwrap(),
        readiness_before
    );
    let _ = std::fs::remove_dir_all(root);
}
