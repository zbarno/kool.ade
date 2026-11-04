use super::*;

#[test]
fn question_store_round_trips_and_rejects_stale_record_revision() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!("planning-questions-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let api = QuestionStore::new(&store);
    let mut question = propose_questions(vec![proposal("data owner", "Who owns it?")], &[])
        .unwrap()
        .remove(0);
    api.save(&question, 0).unwrap();
    let (created, _) = store
        .read_record::<serde_json::Value>(&format!("state/items/{}.json", question.uid))
        .unwrap();
    let created_at_ms = created["createdAtMs"].as_u64().unwrap();
    assert!(created_at_ms > 0);
    let (loaded, revision) = api.load(&question.uid).unwrap();
    assert_eq!(loaded, question);
    assert_eq!(revision, 1);
    question.revision = 1;
    api.save(&question, revision).unwrap();
    let (updated, _) = store
        .read_record::<serde_json::Value>(&format!("state/items/{}.json", question.uid))
        .unwrap();
    assert_eq!(updated["createdAtMs"].as_u64(), Some(created_at_ms));
    assert!(updated["updatedAtMs"].as_u64().unwrap() > 0);
    assert!(api.save(&question, revision).is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn question_records_in_the_removed_collection_are_not_imported() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!(
        "planning-question-no-import-{}",
        uuid::Uuid::new_v4()
    ));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let question = propose_questions(vec![proposal("data owner", "Who owns it?")], &[])
        .unwrap()
        .remove(0);
    let created_at = 1_700_000_000_000_u64;
    let removed_collection_record = serde_json::json!({
        "schemaVersion": 1,
        "uid": question.uid.clone(),
        "createdAtMs": created_at,
        "updatedAtMs": created_at,
        "revision": 1,
        "data": question.clone(),
    });
    let old_path = format!("state/questions/{}.json", question.uid);
    store
        .save_record(&old_path, &removed_collection_record, 0)
        .unwrap();
    let api = QuestionStore::new(&store);
    assert!(api.load(&question.uid).is_err());
    assert!(api.list().unwrap().is_empty());
    assert!(
        store.read(&old_path).is_ok(),
        "old data is never imported or rewritten"
    );
    assert!(
        store
            .read(format!("planning/items/{}.md", question.uid))
            .is_err()
    );
    let _ = std::fs::remove_dir_all(root);
}
