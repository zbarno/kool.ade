use super::*;
use crate::{
    artifacts::planning_store::{PlanningStore, StoreError, StoreMode, paths},
    domain::{Authority, ItemKind, ItemStatus, OpenItem, Priority},
};

fn root() -> (std::path::PathBuf, PlanningStore) {
    let root = std::env::temp_dir().join(format!("koolade-item-records-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    (root, store)
}

fn item(id: &str, question: &str) -> OpenItem {
    OpenItem::new(
        id.into(),
        Priority::High,
        ItemKind::Question,
        "Product".into(),
        Some("Product team".into()),
        question.into(),
        "Captured during planning".into(),
    )
}

fn save(store: &PlanningStore, open: &[OpenItem], resolved: &[OpenItem]) -> Result<(), StoreError> {
    let (changes, checks, migrating) = record_changes(store, open, resolved).unwrap();
    if migrating {
        let expected = store.revision().unwrap();
        store
            .transaction_with_revision_and_record_revisions(&changes, Some(&expected), &checks)
            .map(|_| ())
    } else {
        store
            .transaction_with_record_revisions(&changes, &checks)
            .map(|_| ())
    }
}

#[test]
fn legacy_open_and_resolved_items_migrate_to_content_and_state_records() {
    let (root, store) = root();
    let mut open = item("CLR-101", "Which regions are in scope?");
    open.uid = None;
    open.feature_id = Some("F-101".into());
    let mut resolved = item("CLR-102", "Should old data be retained?");
    resolved.status = ItemStatus::Resolved;
    resolved.authority = Authority::Review;
    let open_path = store.layout().open_items();
    let resolved_path = store.layout().resolved_items();
    std::fs::create_dir_all(open_path.parent().unwrap()).unwrap();
    std::fs::write(
        &open_path,
        crate::artifacts::items_io::serialize(&[open.clone()]),
    )
    .unwrap();
    std::fs::write(
        &resolved_path,
        serde_json::to_vec(&[resolved.clone()]).unwrap(),
    )
    .unwrap();
    let original_open = std::fs::read(&open_path).unwrap();
    let original_resolved = std::fs::read(&resolved_path).unwrap();

    let (loaded_open, loaded_resolved, needs_migration) = load_store(&store).unwrap();
    assert!(needs_migration);
    assert_eq!(loaded_open, vec![open.clone()]);
    assert_eq!(loaded_resolved, vec![resolved.clone()]);
    save(&store, &loaded_open, &loaded_resolved).unwrap();

    assert_eq!(std::fs::read(open_path).unwrap(), original_open);
    assert_eq!(std::fs::read(resolved_path).unwrap(), original_resolved);
    let (loaded_open, loaded_resolved, needs_migration) = load_store(&store).unwrap();
    assert!(!needs_migration);
    assert_eq!(loaded_open[0].id, "CLR-101");
    assert_eq!(loaded_open[0].feature_id.as_deref(), Some("F-101"));
    assert!(loaded_open[0].uid.is_some());
    assert_eq!(loaded_resolved[0], resolved);

    let uid = loaded_open[0].uid.as_ref().unwrap();
    let state_path = store.layout().item_state(uid).unwrap();
    let content_path = store.layout().item_content(uid).unwrap();
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(state_path).unwrap()).unwrap();
    let content = std::fs::read_to_string(content_path).unwrap();
    assert_eq!(state["schemaVersion"], 1);
    assert_eq!(state["uid"].as_str(), Some(uid.as_str()));
    assert_eq!(state["revision"], 1);
    assert!(state["createdAtMs"].is_u64());
    assert!(content.contains("Which regions are in scope?"));
    assert!(!content.contains("**Priority:**"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unrelated_item_records_update_independently_and_stale_same_item_is_rejected() {
    let (root, store) = root();
    let first = item("CLR-201", "First question?");
    let second = item("CLR-202", "Second question?");
    save(&store, &[first.clone(), second.clone()], &[]).unwrap();
    let (open, _, _) = load_store(&store).unwrap();
    let stale_first = open
        .iter()
        .find(|item| item.id == first.id)
        .unwrap()
        .clone();
    let stale_second = open
        .iter()
        .find(|item| item.id == second.id)
        .unwrap()
        .clone();

    let mut first_snapshot = open.clone();
    let mut first_update = stale_first.clone();
    first_update.question = "First operator updated this.".into();
    first_snapshot
        .iter_mut()
        .find(|item| item.id == first_update.id)
        .unwrap()
        .question = first_update.question.clone();
    save(&store, &first_snapshot, &[]).unwrap();

    let mut second_snapshot = open.clone();
    let mut second_update = stale_second.clone();
    second_update.reason = "Second operator updated this.".into();
    second_snapshot
        .iter_mut()
        .find(|item| item.id == second_update.id)
        .unwrap()
        .reason = second_update.reason.clone();
    save(&store, &second_snapshot, &[]).unwrap();
    let (merged, _, _) = load_store(&store).unwrap();
    assert!(
        merged
            .iter()
            .any(|item| item.question == "First operator updated this.")
    );
    assert!(
        merged
            .iter()
            .any(|item| item.reason == "Second operator updated this.")
    );

    let mut stale_snapshot = open;
    let mut stale_update = stale_first;
    stale_update.question = "Stale overwrite.".into();
    stale_snapshot
        .iter_mut()
        .find(|item| item.id == stale_update.id)
        .unwrap()
        .question = stale_update.question;
    let (changes, checks, _) = record_changes(&store, &stale_snapshot, &[]).unwrap();
    assert!(matches!(
        store.transaction_with_record_revisions(&changes, &checks),
        Err(StoreError::StaleRecordRevision { .. })
    ));
    let (merged, _, _) = load_store(&store).unwrap();
    assert!(
        merged
            .iter()
            .any(|item| item.question == "First operator updated this.")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_record_fails_without_falling_back_to_legacy_aggregate() {
    let (root, store) = root();
    let record = item("CLR-301", "A durable question?");
    save(&store, std::slice::from_ref(&record), &[]).unwrap();
    let uid = record.uid.unwrap();
    let path = format!("{}/{uid}.json", paths::ITEM_STATES);
    store.atomic_write(path, b"{broken").unwrap();
    assert!(load_store(&store).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
