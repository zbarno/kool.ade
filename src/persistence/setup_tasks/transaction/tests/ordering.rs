use super::*;

#[test]
fn concurrent_reconciliation_cannot_replace_a_newer_observation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.json");
    let older_path = path.clone();
    let newer_path = path.clone();
    let older = std::thread::spawn(move || reconcile_at(&older_path, &snapshot(10, "older scan")));
    let newer = std::thread::spawn(move || reconcile_at(&newer_path, &snapshot(20, "newer scan")));
    let _ = older.join().unwrap();
    let _ = newer.join().unwrap();

    let store = load_from(&path).unwrap();
    assert_eq!(store.tasks["setup:docker:rootless"].title, "newer scan");
}

#[test]
fn no_op_scan_persists_freshness_before_an_older_scan_arrives() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.json");
    reconcile_at(&path, &snapshot(10, "same finding")).unwrap();
    reconcile_at(&path, &snapshot(20, "same finding")).unwrap();

    assert!(reconcile_at(&path, &snapshot(19, "older finding")).is_err());
    let store = load_from(&path).unwrap();
    assert_eq!(store.last_observed_at_unix_ms, 20);
    assert_eq!(store.last_scan_started_at_unix_ms, 20);
    assert_eq!(store.tasks["setup:docker:rootless"].title, "same finding");
    assert_eq!(store.tasks["setup:docker:rootless"].events.len(), 1);
}
