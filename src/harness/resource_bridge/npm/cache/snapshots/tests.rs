use super::super::persistent_cache_at;
use super::publish_index_snapshot;
use std::fs;

#[test]
fn npm_index_snapshots_live_with_their_temporary_owner() {
    let root = std::env::temp_dir().join(format!(
        "koolade-npm-snapshot-lifecycle-{}",
        uuid::Uuid::new_v4()
    ));
    let state = root.join("state");
    let repository = root.join("repository");
    fs::create_dir_all(&repository).unwrap();
    let cache = persistent_cache_at(&state, &repository).unwrap();
    assert!(!cache.join("npm-index-snapshots").exists());

    let owner = root.join("bridge-run");
    fs::create_dir(&owner).unwrap();
    let snapshots = owner.join("npm-index-snapshots");
    publish_index_snapshot(&cache, &snapshots).unwrap();
    assert!(snapshots.join("current").is_file());
    assert!(!cache.join("npm-index-snapshots").exists());

    fs::remove_dir_all(owner).unwrap();
    assert!(!snapshots.exists());
    fs::remove_dir_all(root).unwrap();
}
