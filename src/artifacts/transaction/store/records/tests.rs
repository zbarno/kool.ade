use super::*;
use crate::artifacts::planning_store::RecordRevisionCheck;

#[test]
fn unrelated_record_writes_do_not_conflict_but_same_record_revision_does() {
    let root =
        std::env::temp_dir().join(format!("koolade-record-revision-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        &root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let first_uid = uuid::Uuid::new_v4();
    let second_uid = uuid::Uuid::new_v4();
    let first_path = format!("state/work/{first_uid}.json");
    let second_path = format!("state/work/{second_uid}.json");

    let (first_paths, _) = store
        .transaction_with_record_revisions(
            &[(first_path.clone(), record(first_uid, 1))],
            &[check(&first_path, 0)],
        )
        .unwrap();
    assert_eq!(first_paths, vec![first_path.clone()]);

    let (second_paths, _) = store
        .transaction_with_record_revisions(
            &[(second_path.clone(), record(second_uid, 1))],
            &[check(&second_path, 0)],
        )
        .unwrap();
    assert_eq!(second_paths, vec![second_path.clone()]);

    let error = store
        .transaction_with_record_revisions(
            &[(first_path.clone(), record(first_uid, 1))],
            &[check(&first_path, 0)],
        )
        .unwrap_err();
    assert!(matches!(
        error,
        StoreError::StaleRecordRevision {
            expected: 0,
            actual: 1,
            ..
        }
    ));
    assert!(
        store
            .transaction_with_record_revisions(
                &[(first_path.clone(), record(first_uid, 2))],
                &[check(&first_path, 1)],
            )
            .is_ok()
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn distinct_record_writes_keep_the_unrelated_store_snapshot_valid() {
    let root = std::env::temp_dir().join(format!(
        "koolade-record-store-revision-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        &root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let snapshot = store.revision().unwrap();
    let first_uid = uuid::Uuid::new_v4();
    let second_uid = uuid::Uuid::new_v4();
    let first_path = format!("state/work/{first_uid}.json");
    let second_path = format!("state/work/{second_uid}.json");

    store
        .transaction_with_revision_and_record_revisions(
            &[(first_path.clone(), record(first_uid, 1))],
            Some(&snapshot),
            &[check(&first_path, 0)],
        )
        .unwrap();
    assert_eq!(store.revision().unwrap(), snapshot);
    store
        .transaction_with_revision_and_record_revisions(
            &[(second_path.clone(), record(second_uid, 1))],
            Some(&snapshot),
            &[check(&second_path, 0)],
        )
        .unwrap();
    assert!(store.read(&first_path).is_ok());
    assert!(store.read(&second_path).is_ok());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn consistent_collection_read_waits_for_multi_file_transaction() {
    use std::sync::mpsc;

    let root =
        std::env::temp_dir().join(format!("koolade-record-read-lock-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        &root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    store.atomic_write("planning/a.md", b"before a").unwrap();
    store.atomic_write("planning/b.md", b"before b").unwrap();
    let reader_store = store.clone();
    let (read_locked_tx, read_locked_rx) = mpsc::channel();
    let (writer_started_tx, writer_started_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        reader_store.with_consistent_read(|| {
            let before = (
                reader_store.read("planning/a.md")?,
                reader_store.read("planning/b.md")?,
            );
            read_locked_tx.send(()).unwrap();
            writer_started_rx.recv().unwrap();
            let after = (
                reader_store.read("planning/a.md")?,
                reader_store.read("planning/b.md")?,
            );
            assert_eq!(after, before);
            Ok::<_, StoreError>(())
        })
    });
    read_locked_rx.recv().unwrap();

    let writer_store = store.clone();
    let writer = std::thread::spawn(move || {
        writer_started_tx.send(()).unwrap();
        writer_store
            .transaction(
                &[
                    ("planning/a.md".into(), b"after a".to_vec()),
                    ("planning/b.md".into(), b"after b".to_vec()),
                ],
                None,
            )
            .unwrap();
    });
    reader.join().unwrap().unwrap();
    writer.join().unwrap();
    assert_eq!(store.read("planning/a.md").unwrap(), b"after a");
    assert_eq!(store.read("planning/b.md").unwrap(), b"after b");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn record_revision_checks_require_a_valid_record_path_and_envelope() {
    let root = std::env::temp_dir().join(format!("koolade-record-shape-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        &root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let uid = uuid::Uuid::new_v4();
    let path = format!("state/work/{uid}.json");

    assert!(
        store
            .transaction_with_record_revisions(
                &[("state/work/all.json".into(), record(uid, 1))],
                &[check("state/work/all.json", 0)],
            )
            .is_err()
    );
    assert!(
        store
            .transaction_with_record_revisions(
                &[(path.clone(), br#"{"uid":"wrong","revision":1}"#.to_vec())],
                &[check(&path, 0)],
            )
            .is_err()
    );
    assert!(matches!(
        store.read(&path),
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound
    ));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn typed_record_helpers_load_list_and_save_one_uid() {
    let root = std::env::temp_dir().join(format!("koolade-record-api-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        &root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let uid = uuid::Uuid::new_v4();
    let path = format!("state/work/{}.json", uid.hyphenated());
    let first = serde_json::json!({
        "schemaVersion": 1,
        "uid": uid.hyphenated().to_string(),
        "revision": 1,
        "createdAtMs": 1,
        "updatedAtMs": 1,
        "data": {"status": "todo"}
    });
    store.save_record(&path, &first, 0).unwrap();
    let (loaded, revision): (serde_json::Value, u64) = store.read_record(&path).unwrap();
    assert_eq!(revision, 1);
    assert_eq!(loaded["data"]["status"], "todo");
    assert_eq!(store.list_record_paths("state/work").unwrap(), vec![path]);

    std::fs::remove_dir_all(root).unwrap();
}

fn check(path: &str, expected_revision: u64) -> RecordRevisionCheck {
    RecordRevisionCheck {
        path: path.to_owned(),
        expected_revision,
    }
}

fn record(uid: uuid::Uuid, revision: u64) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 1,
        "uid": uid.hyphenated().to_string(),
        "revision": revision,
        "createdAtMs": 1,
        "updatedAtMs": revision,
    }))
    .unwrap()
}
