//! Lockfile-driven npm cache preparation through the mediated registry fetcher.
use super::ResourceResponse;
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicUsize,
};

mod adapter;
mod addition;
mod cache;
mod lockfile;
mod manifest;
mod preparation;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

pub(super) use adapter::Adapter;
pub(super) use preparation::SharedPreparationOperations;
use preparation::prepare_with_registry;
use preparation::{PreparationOperations, PreparationRequest, prepare_with_registry_and_ops};
#[cfg(test)]
pub(super) use test_support::{test_addition_preparation_operations, test_preparation_operations};

const MAX_LOCKED_PACKAGES: usize = lockfile::MAX_LOCKED_PACKAGES;

pub(super) fn package_identities(
    root: &Path,
) -> anyhow::Result<Vec<crate::harness::DependencyPackageIdentity>> {
    lockfile::package_identities(root)
}

pub(super) fn packages_from_bytes(
    bytes: &[u8],
) -> anyhow::Result<Vec<crate::harness::DependencyPackageIdentity>> {
    lockfile::packages_from_bytes(bytes)
}

pub(super) fn dependency_manifest_inputs(
    root: &Path,
) -> anyhow::Result<Vec<(String, serde_json::Value)>> {
    manifest::dependency_inputs(root)
}

pub(super) fn dependency_manifest_inputs_from_bytes(
    bytes: &[u8],
) -> anyhow::Result<serde_json::Value> {
    manifest::dependency_input_from_bytes(bytes)
}

pub(super) fn publish_index_snapshot(
    cache_root: &Path,
    snapshot_root: &Path,
) -> anyhow::Result<()> {
    cache::publish_index_snapshot(cache_root, snapshot_root)
}

#[cfg(test)]
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(a >> 2) as usize] as char);
        output.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

pub(super) fn persistent_cache_at(
    state_root: &Path,
    task_repository: &Path,
) -> anyhow::Result<PathBuf> {
    cache::persistent_cache_at(state_root, task_repository)
}

#[cfg(test)]
pub(super) fn verified_offline_cache(worktree: &Path, cache: &Path) -> bool {
    lockfile::collect_lockfiles(worktree).is_ok_and(|packages| {
        packages.iter().all(|package| {
            lockfile::npm_cache_digest_path(cache, &package.integrity)
                .is_some_and(|path| lockfile::verify_sha512_file(&path, &package.integrity))
        })
    })
}

pub(super) fn verified_lock_identity(worktree: &Path, package: &str, version: &str) -> bool {
    lockfile::contains_verified_identity(worktree, package, version)
}

pub(super) fn prepare_addition(
    worktree: &Path,
    response_dir: &Path,
    npm_cache: &Path,
    npm_snapshot: &Path,
    need: &crate::harness::DependencyNeed,
    decision: crate::harness::DependencyDecision,
    downloaded_bytes: &AtomicUsize,
) -> anyhow::Result<ResourceResponse> {
    addition::prepare(
        worktree,
        response_dir,
        npm_cache,
        npm_snapshot,
        need,
        decision,
        downloaded_bytes,
    )
}

pub(super) fn prepare(
    worktree: &Path,
    response_dir: &Path,
    npm_cache: &Path,
    npm_snapshot: &Path,
    purpose: &str,
    downloaded_bytes: &AtomicUsize,
) -> anyhow::Result<ResourceResponse> {
    prepare_with_registry(
        worktree,
        response_dir,
        npm_cache,
        npm_snapshot,
        purpose,
        downloaded_bytes,
        None,
    )
}

pub(super) fn prepare_for_addition(
    worktree: &Path,
    response_dir: &Path,
    npm_cache: &Path,
    npm_snapshot: &Path,
    registry: &url::Url,
    purpose: &str,
    downloaded_bytes: &AtomicUsize,
) -> anyhow::Result<ResourceResponse> {
    prepare_with_registry(
        worktree,
        response_dir,
        npm_cache,
        npm_snapshot,
        purpose,
        downloaded_bytes,
        Some(registry),
    )
}

fn prepare_for_addition_with_ops(
    request: PreparationRequest<'_>,
    operations: PreparationOperations<'_>,
) -> anyhow::Result<ResourceResponse> {
    prepare_with_registry_and_ops(request, operations)
}
