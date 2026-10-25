use super::*;

#[test]
fn a_later_reserved_generation_fences_an_older_scan_that_finishes_last() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.json");
    let slow_generation = begin_scan_at(&path).unwrap();
    let fast_generation = begin_scan_at(&path).unwrap();

    let fast = snapshot(fast_generation, "newer reserved scan");
    reconcile_live_at(&path, &fast, fast_generation).unwrap();

    let slow = snapshot(slow_generation, "older reserved scan");
    assert!(reconcile_live_at(&path, &slow, slow_generation).is_err());
    let store = load_from(&path).unwrap().0;
    assert_eq!(
        store.tasks["setup:docker:rootless"].title,
        "newer reserved scan"
    );
}
