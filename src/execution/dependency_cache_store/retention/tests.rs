use super::*;

#[test]
fn byte_limit_eviction_selects_oldest_snapshots_until_the_incoming_entry_fits() {
    let old = PathBuf::from("old");
    let newer = PathBuf::from("newer");
    let snapshot_bytes = 700 * 1024 * 1024;
    let entries = vec![
        SnapshotEntry {
            directory: newer.clone(),
            last_used_millis: 2,
            stored_bytes: snapshot_bytes,
        },
        SnapshotEntry {
            directory: old.clone(),
            last_used_millis: 1,
            stored_bytes: snapshot_bytes,
        },
    ];

    let evicted = eviction_candidates(entries, snapshot_bytes * 2, 100 * 1024 * 1024).unwrap();

    assert_eq!(evicted, vec![old]);
}
