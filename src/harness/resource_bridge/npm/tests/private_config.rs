use super::base64;
use sha2::{Digest, Sha512};
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

#[test]
fn private_configuration_allows_verified_npm_cache_hits_without_downloading() {
    let root = std::env::temp_dir().join(format!(
        "koolade-npm-private-cache-{}",
        uuid::Uuid::new_v4()
    ));
    let worktree = root.join("worktree");
    let response_dir = root.join("resources");
    let cache = root.join("npm-cache");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&response_dir).unwrap();
    fs::create_dir_all(&cache).unwrap();

    let archive = b"verified cached npm package";
    let integrity = format!("sha512-{}", base64(&Sha512::digest(archive)));
    let cached_archive = super::super::lockfile::npm_cache_digest_path(&cache, &integrity).unwrap();
    fs::create_dir_all(cached_archive.parent().unwrap()).unwrap();
    fs::write(cached_archive, archive).unwrap();
    fs::write(
        worktree.join("package-lock.json"),
        serde_json::to_vec(&serde_json::json!({
            "lockfileVersion": 3,
            "packages": {
                "node_modules/synthetic-helper": {
                    "version": "1.0.0",
                    "resolved": "https://registry.npmjs.org/synthetic-helper/-/synthetic-helper-1.0.0.tgz",
                    "integrity": integrity,
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let registry = url::Url::parse("https://registry.npmjs.org/").unwrap();
    let downloads = AtomicUsize::new(0);
    let response = super::super::prepare_with_registry_and_ops(
        super::super::PreparationRequest {
            worktree: &worktree,
            response_dir: &response_dir,
            npm_cache: &cache,
            npm_snapshot: &response_dir.join("npm-index-snapshots"),
            purpose: "Restore the locked synthetic npm dependency",
            downloaded_bytes: &downloads,
            authorized_registry: Some(&registry),
            allow_downloads: false,
        },
        super::super::PreparationOperations {
            retrieve: &|_, _, _, _, _| {
                downloads.fetch_add(1, Ordering::Relaxed);
                anyhow::bail!("private configuration must not allow a download")
            },
            index_cache: &|_, _, _, _| {
                panic!("a verified cache hit must not require cache indexing")
            },
        },
    )
    .unwrap();

    assert_eq!(response.status, "prepared");
    let telemetry = response.preparation.unwrap();
    assert_eq!(
        telemetry.status,
        Some(crate::harness::DependencyPreparationStatus::AlreadyAvailable)
    );
    assert_eq!(telemetry.cache_hits, 1);
    assert_eq!(telemetry.bytes_downloaded, 0);
    assert_eq!(downloads.load(Ordering::Relaxed), 0);
    fs::remove_dir_all(root).unwrap();
}
