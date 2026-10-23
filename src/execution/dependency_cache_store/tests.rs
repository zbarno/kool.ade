use super::*;

fn snapshot() -> CacheSnapshot {
    let source = tempfile::tempdir().unwrap();
    fs::write(source.path().join("index.cache"), b"verified packages").unwrap();
    CacheSnapshot::capture(
        source.path(),
        "11111111-1111-4111-8111-111111111111",
        crate::execution::contract::DependencyCacheEcosystemV1::Npm,
        "22222222-2222-4222-8222-222222222222",
        "sha256:lockfile",
        "amd64",
        "local",
    )
    .unwrap()
}

#[test]
fn stored_cache_snapshot_round_trips_only_for_its_dependency_request_and_ref() {
    let root = tempfile::tempdir().unwrap();
    let store = DependencyCacheSnapshotStore::new(root.path().join("snapshots"));
    let snapshot = snapshot();

    let request_id = "dependency-33333333-3333-4333-8333-333333333333";
    store.put(request_id, &snapshot).unwrap();
    let restored = store.get(request_id, &snapshot.cache_ref).unwrap();
    assert_eq!(restored.archive, snapshot.archive);
    assert_eq!(restored.manifest, snapshot.manifest);
    assert!(
        store
            .get(
                "dependency-44444444-4444-4444-8444-444444444444",
                &snapshot.cache_ref
            )
            .is_err()
    );

    let mut changed_ref = snapshot.cache_ref.clone();
    changed_ref.target_id = "other-target".into();
    assert!(store.get(request_id, &changed_ref).is_err());
}

#[test]
fn cache_store_accepts_legacy_uuid_and_rejects_malformed_dependency_request_ids() {
    let root = tempfile::tempdir().unwrap();
    let store = DependencyCacheSnapshotStore::new(root.path().join("snapshots"));
    let snapshot = snapshot();
    let legacy_id = "33333333-3333-4333-8333-333333333333";
    store.put(legacy_id, &snapshot).unwrap();
    assert!(store.get(legacy_id, &snapshot.cache_ref).is_ok());
    assert!(store.put("dependency-not-a-uuid", &snapshot).is_err());
}

#[cfg(windows)]
#[test]
fn stored_cache_snapshot_refreshes_an_existing_access_marker_on_windows() {
    let root = tempfile::tempdir().unwrap();
    let store = DependencyCacheSnapshotStore::new(root.path().join("snapshots"));
    let snapshot = snapshot();
    let request_id = "dependency-33333333-3333-4333-8333-333333333333";

    store.put(request_id, &snapshot).unwrap();
    store.get(request_id, &snapshot.cache_ref).unwrap();
    store.put(request_id, &snapshot).unwrap();
}

#[test]
fn stored_cache_snapshot_rejects_archive_corruption() {
    let root = tempfile::tempdir().unwrap();
    let store = DependencyCacheSnapshotStore::new(root.path().join("snapshots"));
    let snapshot = snapshot();
    let request_id = "dependency-33333333-3333-4333-8333-333333333333";
    store.put(request_id, &snapshot).unwrap();
    let directory = store.snapshot_dir(request_id, &snapshot.cache_ref).unwrap();
    fs::write(directory.join("snapshot.tar"), b"tampered").unwrap();
    assert!(store.get(request_id, &snapshot.cache_ref).is_err());
}

#[cfg(unix)]
#[test]
fn cache_snapshot_store_rejects_a_symlink_root() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let store_root = root.path().join("snapshots");
    symlink(external.path(), &store_root).unwrap();
    let store = DependencyCacheSnapshotStore::new(store_root);
    assert!(
        store
            .put(
                "dependency-33333333-3333-4333-8333-333333333333",
                &snapshot()
            )
            .is_err()
    );
    assert!(
        store
            .get(
                "dependency-33333333-3333-4333-8333-333333333333",
                &snapshot().cache_ref
            )
            .is_err()
    );
}

#[test]
fn cache_snapshot_store_evicts_the_least_recently_used_snapshot_at_capacity() {
    let root = tempfile::tempdir().unwrap();
    let store = DependencyCacheSnapshotStore::new(root.path().join("snapshots"));
    store.ensure_private_root().unwrap();
    for index in 0..MAX_STORED_SNAPSHOTS {
        let directory = store.root.join(format!("{index:064x}"));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("manifest.json"), b"{}").unwrap();
        fs::write(directory.join("snapshot.tar"), b"cache").unwrap();
        fs::write(directory.join(".last-used"), (index as u64).to_be_bytes()).unwrap();
    }
    touch_last_used(&store.root.join(format!("{:064x}", 0))).unwrap();
    store.ensure_capacity_for(12, 128).unwrap();

    assert!(store.root.join(format!("{:064x}", 0)).exists());
    assert!(!store.root.join(format!("{:064x}", 1)).exists());
    assert_eq!(
        fs::read_dir(&store.root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().unwrap().is_dir())
            .count(),
        MAX_STORED_SNAPSHOTS - 1
    );
}

#[test]
fn oversized_cache_snapshot_does_not_evict_existing_snapshots() {
    let root = tempfile::tempdir().unwrap();
    let store = DependencyCacheSnapshotStore::new(root.path().join("snapshots"));
    store.ensure_private_root().unwrap();
    let existing = store.root.join(format!("{:064x}", 0));
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("manifest.json"), b"{}").unwrap();
    fs::write(existing.join("snapshot.tar"), b"cache").unwrap();

    assert!(
        store
            .ensure_capacity_for(MAX_STORED_BYTES as usize, 128)
            .is_err()
    );
    assert!(existing.exists());
}
