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
fn legacy_question_records_load_and_migrate_to_item_paths_on_write() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!(
        "planning-question-migration-{}",
        uuid::Uuid::new_v4()
    ));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let mut question = propose_questions(vec![proposal("data owner", "Who owns it?")], &[])
        .unwrap()
        .remove(0);
    let created_at = 1_700_000_000_000_u64;
    let legacy = serde_json::json!({
        "schemaVersion": 1,
        "uid": question.uid.clone(),
        "createdAtMs": created_at,
        "updatedAtMs": created_at,
        "revision": 1,
        "data": question.clone(),
    });
    let old_path = format!("state/questions/{}.json", question.uid);
    store.save_record(&old_path, &legacy, 0).unwrap();
    let old_bytes = store.read(&old_path).unwrap();

    let api = QuestionStore::new(&store);
    let (loaded, outer_revision) = api.load(&question.uid).unwrap();
    assert_eq!(loaded, question);
    assert_eq!(outer_revision, 1);
    reconcile_answer(
        &mut question,
        0,
        4,
        Some("Use the data steward <script>alert(1)</script>"),
        None,
        "operator",
        created_at + 1,
    )
    .unwrap();
    api.save(&question, outer_revision).unwrap();

    let (migrated, revision) = api.load(&question.uid).unwrap();
    assert_eq!(migrated, question);
    assert_eq!(
        revision, 1,
        "migrated canonical record starts at outer revision one"
    );
    assert_eq!(
        store.read(&old_path).unwrap(),
        old_bytes,
        "legacy record is retained unchanged"
    );
    let markdown = String::from_utf8(
        store
            .read(format!("planning/items/{}.md", question.uid))
            .unwrap(),
    )
    .unwrap();
    assert!(markdown.contains("## Recorded decision"));
    assert!(markdown.contains("Use the data steward &lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!markdown.contains("<script>"));
    assert!(markdown.contains("Answered by: operator"));
    let (open, resolved, _) = crate::artifacts::items_io::load_store(&store).unwrap();
    assert!(
        open.is_empty() && resolved.is_empty(),
        "typed clarifications are not code-running item cards"
    );
    let _ = std::fs::remove_dir_all(root);
}
