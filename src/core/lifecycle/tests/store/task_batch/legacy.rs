use super::super::Fixture;
use crate::core::lifecycle::LifecycleStore;

pub(super) fn verify_fixture(
    fixture: &Fixture,
    store: &crate::artifacts::planning_store::PlanningStore,
    task_uid: &str,
) {
    let record_path = format!("state/lifecycle_tasks/{task_uid}.json");
    let task_file = fixture.0.join(&record_path);
    let mut legacy_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&task_file).unwrap()).unwrap();
    legacy_json["record"]
        .as_object_mut()
        .unwrap()
        .remove("approved_requirement_ids");
    let store_revision = store.revision().unwrap();
    store
        .transaction(
            &[(record_path, serde_json::to_vec(&legacy_json).unwrap())],
            Some(&store_revision),
        )
        .unwrap();

    let reopened_store = fixture.store();
    let lifecycle = LifecycleStore::new(&reopened_store);
    assert!(
        lifecycle
            .load_task(task_uid)
            .unwrap()
            .approved_requirement_ids
            .is_empty()
    );
    assert_eq!(lifecycle.list_tasks().unwrap().len(), 3);
}
