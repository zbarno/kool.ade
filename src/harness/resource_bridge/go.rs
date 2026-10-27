//! go.sum-backed Go module preparation through the approved module proxy.
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
mod adapter;
#[cfg(unix)]
mod cache;
mod lockfile;
#[cfg(unix)]
mod prepare;

#[cfg(unix)]
pub(super) use adapter::Adapter;
#[cfg(unix)]
pub(super) use prepare::SharedPreparationOperations;

pub(super) fn package_identities(
    root: &Path,
) -> anyhow::Result<Vec<crate::harness::DependencyPackageIdentity>> {
    lockfile::package_identities(root)
}

pub(super) fn package_identities_from_contents(
    contents: &str,
) -> anyhow::Result<Vec<crate::harness::DependencyPackageIdentity>> {
    lockfile::package_identities_from_contents(contents)
}

pub(super) fn manifest_inputs(root: &Path) -> anyhow::Result<Vec<(String, serde_json::Value)>> {
    lockfile::manifest_inputs(root)
}

pub(super) fn manifest_inputs_from_bytes(
    manifest: &[u8],
    sums: &[u8],
) -> anyhow::Result<Vec<(String, serde_json::Value)>> {
    lockfile::manifest_inputs_from_bytes(manifest, sums)
}

#[cfg(unix)]
pub(super) fn persistent_cache_at(state_root: &Path) -> anyhow::Result<PathBuf> {
    cache::persistent_cache_at(state_root)
}

#[cfg(unix)]
pub(super) fn snapshot_root(cache_root: &Path, lockfile_identity: &str) -> anyhow::Result<PathBuf> {
    cache::snapshot_root(cache_root, lockfile_identity)
}

#[cfg(all(test, unix))]
pub(crate) fn test_preparation_operations(
    fetch: impl Fn(&Path, &Path, usize) -> (usize, anyhow::Result<()>) + Send + Sync + 'static,
) -> std::sync::Arc<SharedPreparationOperations> {
    prepare::test_preparation_operations(fetch)
}
