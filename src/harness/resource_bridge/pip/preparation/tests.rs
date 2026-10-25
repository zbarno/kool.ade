use super::{prepare_with_download_policy_and_operations, test_preparation_operations};
use crate::harness::resource_bridge::pip::{cache, lockfile};
use sha2::{Digest, Sha256};
use std::{fs, sync::atomic::AtomicUsize};

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fixture() -> (
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
    String,
) {
    let root = std::env::temp_dir().join(format!("koolade-pip-prepare-{}", uuid::Uuid::new_v4()));
    let worktree = root.join("worktree");
    let state = root.join("state");
    let snapshots = root.join("snapshots");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&snapshots).unwrap();
    let content = b"synthetic universal wheel";
    fs::write(
        worktree.join("requirements.txt"),
        format!("demo_pkg==1.2.3 --hash=sha256:{}\n", sha256(content)),
    )
    .unwrap();
    let packages = lockfile::collect(&worktree).unwrap();
    let identity = lockfile::identity(&packages).unwrap();
    (root, worktree, state, snapshots, identity)
}

#[test]
fn prepares_and_publishes_only_the_injected_hash_verified_wheelhouse() {
    let (root, worktree, state, snapshots, identity) = fixture();
    let cache = cache::persistent_cache_at(&state).unwrap();
    let content = b"synthetic universal wheel".to_vec();
    let expected_hash = sha256(&content);
    let operations = test_preparation_operations(move |requirements, wheelhouse, _limit| {
        let pinned = fs::read_to_string(requirements)?;
        anyhow::ensure!(
            pinned.contains(&expected_hash),
            "canonical lock snapshot omitted its hash"
        );
        fs::write(wheelhouse.join("demo_pkg-1.2.3-py3-none-any.whl"), &content)?;
        Ok(content.len())
    });
    let response = prepare_with_download_policy_and_operations(
        &worktree,
        &cache,
        &snapshots,
        &AtomicUsize::new(0),
        true,
        Some(&operations),
        &identity,
    )
    .unwrap();

    assert_eq!(response.status, "prepared");
    assert_eq!(
        response.preparation.as_ref().and_then(|prep| prep.status),
        Some(crate::harness::DependencyPreparationStatus::Prepared)
    );
    assert_eq!(
        cache::snapshot_root(&cache, &identity)
            .unwrap()
            .join("wheels")
            .read_dir()
            .unwrap()
            .count(),
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn private_project_configuration_needs_all_wheels_to_be_prepared_already() {
    let (root, worktree, state, snapshots, identity) = fixture();
    let cache = cache::persistent_cache_at(&state).unwrap();
    let response = prepare_with_download_policy_and_operations(
        &worktree,
        &cache,
        &snapshots,
        &AtomicUsize::new(0),
        false,
        None,
        &identity,
    )
    .unwrap();
    assert_eq!(response.status, "needs_attention");
    fs::remove_dir_all(root).unwrap();
}
