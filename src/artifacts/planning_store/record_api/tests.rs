use crate::artifacts::planning_store::{PlanningStore, StoreError, StoreMode};
use std::sync::mpsc;

#[test]
fn public_record_readers_wait_for_a_consistent_store_snapshot() {
    let root = std::env::temp_dir().join(format!(
        "koolade-record-api-read-lock-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let uid = uuid::Uuid::new_v4();
    let path = format!("state/work/{uid}.json");
    let record = serde_json::json!({
        "schemaVersion": 1,
        "uid": uid.hyphenated().to_string(),
        "revision": 1,
        "createdAtMs": 1,
        "updatedAtMs": 1,
        "data": {"status": "todo"}
    });
    store.save_record(&path, &record, 0).unwrap();

    let (started_tx, started_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let (read_join, list_join) = store
        .with_consistent_read(|| {
            let read_store = store.clone();
            let read_path = path.clone();
            let read_started = started_tx.clone();
            let read_finished = finished_tx.clone();
            let read_join = std::thread::spawn(move || {
                read_started.send(()).unwrap();
                let result: Result<(serde_json::Value, u64), StoreError> =
                    read_store.read_record(&read_path);
                read_finished.send(()).unwrap();
                result
            });

            let list_store = store.clone();
            let list_started = started_tx.clone();
            let list_finished = finished_tx.clone();
            let list_join = std::thread::spawn(move || {
                list_started.send(()).unwrap();
                let result = list_store.list_record_paths("state/work");
                list_finished.send(()).unwrap();
                result
            });

            started_rx.recv().unwrap();
            started_rx.recv().unwrap();
            assert!(
                finished_rx
                    .recv_timeout(std::time::Duration::from_millis(25))
                    .is_err()
            );
            Ok::<_, StoreError>((read_join, list_join))
        })
        .unwrap();

    assert_eq!(read_join.join().unwrap().unwrap().1, 1);
    assert_eq!(list_join.join().unwrap().unwrap(), vec![path]);
    std::fs::remove_dir_all(root).unwrap();
}
