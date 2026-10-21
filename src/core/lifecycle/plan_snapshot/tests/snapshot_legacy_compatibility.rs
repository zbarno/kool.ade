use super::*;

#[test]
fn legacy_snapshot_without_requirement_rationale_or_criterion_links_roundtrips() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};

    let root = std::env::temp_dir().join(format!(
        "legacy-plan-snapshot-context-{}",
        uuid::Uuid::new_v4()
    ));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.schema_version = 1;
    snapshot.original_goal.clear();
    snapshot.acceptance_criteria = vec!["Selected rows are exported.".into()];
    for requirement in &mut snapshot.requirements {
        requirement.rationale.clear();
        requirement.acceptance_criteria_ids.clear();
    }
    snapshot.content_hash = hash::content_hash(&snapshot).unwrap();
    snapshot.uid = snapshot_uid(&snapshot.feature_uid, &snapshot.content_hash).unwrap();
    snapshot.validate().unwrap();

    let uid = snapshot.uid.clone();
    let path = format!("state/lifecycle_plans/{}.json", snapshot.uid);
    let record = serde_json::json!({
        "schemaVersion": 1,
        "uid": snapshot.uid,
        "createdAtMs": 1,
        "updatedAtMs": 1,
        "revision": 1,
        "data": snapshot,
    });
    store
        .transaction(&[(path, serde_json::to_vec(&record).unwrap())], None)
        .unwrap();
    let loaded = crate::core::lifecycle::plan_snapshot::load_persisted(&store, &uid)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.schema_version, 1);
    assert!(loaded.requirements[0].rationale.is_empty());
    assert!(loaded.requirements[0].acceptance_criteria_ids.is_empty());
    assert_eq!(loaded.acceptance_criteria, ["Selected rows are exported."]);
    let _ = std::fs::remove_dir_all(root);
}
