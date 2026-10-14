//! Lockfile-driven Cargo cache preparation through a registry-restricted proxy.
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use super::ResourceResponse;

mod cache;
mod lockfile;
mod prepare;
#[cfg(test)]
mod test_support;
#[cfg(test)]
use prepare::prepare_with_download_policy;
#[cfg(test)]
use prepare::{locate_cargo_in_path, run_fetch};
#[cfg(test)]
pub(in crate::harness::resource_bridge) use test_support::test_preparation_operations;
pub(super) fn package_identities_from_contents(
    contents: &str,
) -> anyhow::Result<Vec<crate::harness::DependencyPackageIdentity>> {
    lockfile::package_identities_from_contents(contents)
}

pub(super) struct Adapter;

impl super::adapter::DependencyAdapter for Adapter {
    fn ecosystem(&self) -> crate::harness::PackageEcosystem {
        crate::harness::PackageEcosystem::Cargo
    }

    fn ecosystem_label(&self) -> &'static str {
        "Cargo"
    }

    fn prepare(
        &self,
        context: &super::adapter::PreparationContext<'_>,
        need: &crate::harness::DependencyNeed,
    ) -> anyhow::Result<ResourceResponse> {
        anyhow::ensure!(
            crate::harness::dependency_decision_allowed(need, context.decision),
            "Cargo dependency request did not pass its exact authorization decision"
        );
        let _cache_gate = context
            .cargo_cache_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Cargo cache coordination lock is unavailable"))?;
        anyhow::ensure!(
            need.kind == crate::harness::DependencyKind::ExistingRestore,
            "Cargo currently supports checksum-backed existing restores only"
        );
        crate::harness::resource_bridge::dependency::validate_lockfile_identity(
            context.worktree,
            need,
        )?;
        self::prepare::prepare_with_download_policy_and_operations(
            context.worktree,
            context.cargo_cache,
            context.downloaded_bytes,
            context.allow_downloads,
            context.cargo_operations,
        )
    }
}

pub(super) fn persistent_cache_at(state_root: &Path) -> anyhow::Result<PathBuf> {
    cache::persistent_cache_at(state_root)
}

pub(super) struct SharedPreparationOperations {
    pub(super) fetch: std::sync::Arc<prepare::CargoFetchOperation>,
}

#[cfg(test)]
pub(super) fn prepare(
    worktree: &Path,
    cargo_cache: &Path,
    downloaded_bytes: &AtomicUsize,
) -> anyhow::Result<ResourceResponse> {
    prepare_with_download_policy(worktree, cargo_cache, downloaded_bytes, true)
}

#[cfg(unix)]
pub(super) fn ensure_private_dir(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Cargo cache path contains a symlink or non-directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(path) {
            Ok(()) => {}
            Err(create_error) if create_error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(path)?;
                anyhow::ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "Cargo cache path contains a symlink or non-directory"
                );
            }
            Err(create_error) => return Err(create_error.into()),
        },
        Err(error) => return Err(error.into()),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn ensure_private_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)?;
    Ok(())
}

#[cfg(test)]
mod tests;
