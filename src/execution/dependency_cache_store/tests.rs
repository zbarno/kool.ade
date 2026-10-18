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

    let request_id = "33333333-3333-4333-8333-333333333333";
    store.put(request_id, &snapshot).unwrap();
    let restored = store.get(request_id, &snapshot.cache_ref).unwrap();
    assert_eq!(restored.archive, snapshot.archive);
    assert_eq!(restored.manifest, snapshot.manifest);
    assert!(
        store
            .get("44444444-4444-4444-8444-444444444444", &snapshot.cache_ref)
            .is_err()
    );

    let mut changed_ref = snapshot.cache_ref.clone();
    changed_ref.target_id = "other-target".into();
    assert!(store.get(request_id, &changed_ref).is_err());
}

#[test]
fn stored_cache_snapshot_rejects_archive_corruption() {
    let root = tempfile::tempdir().unwrap();
    let store = DependencyCacheSnapshotStore::new(root.path().join("snapshots"));
    let snapshot = snapshot();
    let request_id = "33333333-3333-4333-8333-333333333333";
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
            .put("33333333-3333-4333-8333-333333333333", &snapshot())
            .is_err()
    );
    assert!(
        store
            .get(
                "33333333-3333-4333-8333-333333333333",
                &snapshot().cache_ref
            )
            .is_err()
    );
}

#[test]
fn cache_snapshot_store_refuses_more_than_its_bounded_retention_capacity() {
    let root = tempfile::tempdir().unwrap();
    let store = DependencyCacheSnapshotStore::new(root.path().join("snapshots"));
    store.ensure_private_root().unwrap();
    for index in 0..MAX_STORED_SNAPSHOTS {
        let directory = store.root.join(format!("{index:064x}"));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("manifest.json"), b"{}").unwrap();
        fs::write(directory.join("snapshot.tar"), b"cache").unwrap();
    }
    assert!(store.ensure_capacity_for(12, 128).is_err());
}
