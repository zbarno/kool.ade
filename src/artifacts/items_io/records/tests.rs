use super::*;
use crate::{
    artifacts::planning_store::{PlanningStore, StoreError, StoreMode, paths},
    domain::{ItemKind, OpenItem, Priority},
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
        question.into(),
        "Captured during planning".into(),
    )
}

fn save(store: &PlanningStore, open: &[OpenItem], resolved: &[OpenItem]) -> Result<(), StoreError> {
    let (changes, checks) = record_changes(store, open, resolved).unwrap();
    store
        .transaction_with_record_revisions(&changes, &checks)
        .map(|_| ())
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
    let (changes, checks) = record_changes(&store, &stale_snapshot, &[]).unwrap();
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

#[test]
fn task_intent_item_link_requires_exactly_one_persisted_root() {
    let (root_dir, store) = root();
    let mut root = crate::core::planning_work::Work::new(
        "task:intent-root".into(),
        "Original goal".into(),
        "Preserve this request".into(),
        "Planning".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    crate::core::planning_work::save(&store, std::slice::from_ref(&root)).unwrap();

    let mut scoped_item = item("CLR-401", "Should this belong to the request?");
    scoped_item.task_intent_uid = Some(root.uid.clone());
    scoped_item.task_intent_cycle_uid = Some(root.uid.clone());
    save(&store, std::slice::from_ref(&scoped_item), &[]).unwrap();
    let (open, _, _) = load_store(&store).unwrap();
    assert_eq!(open[0].task_intent_uid.as_deref(), Some(root.uid.as_str()));
    assert_eq!(
        open[0].task_intent_cycle_uid.as_deref(),
        Some(root.uid.as_str())
    );

    let missing_uid = uuid::Uuid::new_v4().to_string();
    scoped_item.task_intent_uid = Some(missing_uid);
    let error = record_changes(&store, &[scoped_item.clone()], &[])
        .unwrap_err()
        .to_string();
    assert!(error.contains("exactly one valid root"), "{error}");

    scoped_item.task_intent_uid = Some(root.uid.clone());
    scoped_item.task_intent_cycle_uid = Some(uuid::Uuid::new_v4().to_string());
    let error = record_changes(&store, &[scoped_item], &[])
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown TaskIntent cycle"), "{error}");
    std::fs::remove_dir_all(root_dir).unwrap();
}

#[test]
fn persisted_task_intent_item_link_rejects_a_missing_root() {
    let (root_dir, store) = root();
    let mut scoped_item = item("CLR-402", "Which request owns this decision?");
    scoped_item.task_intent_uid = None;
    save(&store, std::slice::from_ref(&scoped_item), &[]).unwrap();

    let uid = scoped_item.uid.as_ref().unwrap();
    let path = format!("{}/{uid}.json", paths::ITEM_STATES);
    let mut record: serde_json::Value =
        serde_json::from_slice(&store.read(&path).unwrap()).unwrap();
    record["data"]["taskIntentUid"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
    store
        .atomic_write(path, &serde_json::to_vec(&record).unwrap())
        .unwrap();

    let error = load_store(&store).unwrap_err().to_string();
    assert!(
        error.contains("TaskIntent root does not exist")
            || error.contains("exactly one valid root"),
        "{error}"
    );
    std::fs::remove_dir_all(root_dir).unwrap();
}

#[test]
fn duplicate_task_intent_root_identity_is_rejected_before_persistence() {
    let (root_dir, store) = root();
    let mut root = crate::core::planning_work::Work::new(
        "task:intent-root".into(),
        "Original goal".into(),
        "Preserve this request".into(),
        "Planning".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    let (changes, checks) =
        crate::core::planning_work::record_changes(&store, &[root.clone(), root]).unwrap();
    let error = store
        .transaction_with_record_revisions(&changes, &checks)
        .unwrap_err()
        .to_string();
    assert!(error.contains("duplicate"), "{error}");
    std::fs::remove_dir_all(root_dir).unwrap();
}
