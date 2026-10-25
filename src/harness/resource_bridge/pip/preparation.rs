use super::{cache, lockfile};
use crate::harness::resource_bridge::ResourceResponse;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicUsize},
};

mod host;

pub(in crate::harness::resource_bridge) struct DownloadOutcome {
    pub(super) bytes_received: usize,
    pub(super) result: anyhow::Result<()>,
}

pub(in crate::harness::resource_bridge) type PipDownloadOperation =
    dyn Fn(&Path, &Path, usize) -> DownloadOutcome + Send + Sync;

pub(in crate::harness::resource_bridge) struct SharedPreparationOperations {
    pub(super) download: Arc<PipDownloadOperation>,
}

pub(super) fn prepare_with_download_policy_and_operations(
    worktree: &Path,
    pip_cache: &Path,
    pip_snapshot: &Path,
    downloaded_bytes: &AtomicUsize,
    allow_downloads: bool,
    operations: Option<&SharedPreparationOperations>,
    expected_identity: &str,
) -> anyhow::Result<ResourceResponse> {
    let packages = lockfile::collect(worktree)?;
    anyhow::ensure!(
        lockfile::identity(&packages)? == expected_identity,
        "The authorized requirements.txt identity changed before package preparation"
    );
    if packages.is_empty() {
        return Ok(ResourceResponse::prepared(
            "requirements.txt contains no packages to retrieve".into(),
        )
        .with_preparation(crate::harness::DependencyPreparationTelemetry {
            status: Some(crate::harness::DependencyPreparationStatus::AlreadyAvailable),
            ..Default::default()
        }));
    }
    let cache_hits = cache::verified_wheel_count(pip_cache, expected_identity, &packages)?;
    if !allow_downloads && cache_hits != packages.len() {
        return Ok(ResourceResponse::needs_attention(
            "Fresh PyPI downloads are disabled while private project configuration is mounted; one or more hash-locked wheels are missing from the app-owned cache.".into(),
        ));
    }
    if cache_hits == packages.len() {
        return Ok(prepared_response(packages.len(), cache_hits, 0, true));
    }

    let stage = create_staging_directory(pip_snapshot)?;
    let result = prepare_missing(
        pip_cache,
        &stage,
        downloaded_bytes,
        operations,
        expected_identity,
        &packages,
        cache_hits,
    );
    let _ = fs::remove_dir_all(&stage);
    result
}

fn prepare_missing(
    pip_cache: &Path,
    stage: &Path,
    downloaded_bytes: &AtomicUsize,
    operations: Option<&SharedPreparationOperations>,
    identity: &str,
    packages: &[lockfile::LockedPackage],
    cache_hits: usize,
) -> anyhow::Result<ResourceResponse> {
    let requirements = stage.join("requirements.txt");
    lockfile::write_canonical_snapshot(&requirements, packages)?;
    let wheels = stage.join("wheels");
    create_private_dir(&wheels)?;
    let reservation = crate::harness::resource_bridge::budget::reserve_downloads(
        downloaded_bytes,
        crate::harness::resource_bridge::MAX_SESSION_BYTES,
        crate::harness::resource_bridge::MAX_SESSION_BYTES,
    )?;
    let outcome = match operations {
        Some(operations) => (operations.download)(&requirements, &wheels, reservation),
        None => host::fetch_from_pypi(&requirements, &wheels, reservation),
    };
    crate::harness::resource_bridge::budget::settle_downloads(
        downloaded_bytes,
        reservation,
        outcome.bytes_received,
    );
    let bytes_downloaded = outcome.bytes_received;
    outcome.result?;
    anyhow::ensure!(
        cache::verify_wheelhouse(&wheels, packages)? == packages.len(),
        "Pip did not prepare every hash-locked wheel"
    );
    cache::publish_wheelhouse(pip_cache, identity, &wheels, packages)?;
    let published = cache::verified_wheel_count(pip_cache, identity, packages)?;
    anyhow::ensure!(
        published == packages.len(),
        "Pip cache publication failed independent wheel verification"
    );
    Ok(prepared_response(
        packages.len(),
        cache_hits,
        bytes_downloaded,
        false,
    ))
}

fn prepared_response(
    package_count: usize,
    cache_hits: usize,
    bytes_downloaded: usize,
    already_available: bool,
) -> ResourceResponse {
    let summary = if already_available {
        format!(
            "Pip verified all {package_count} hash-locked wheels in the app-owned cache; no downloads were needed."
        )
    } else {
        format!(
            "Pip prepared {package_count} SHA-256-verified wheels from requirements.txt; the sandbox remains offline."
        )
    };
    ResourceResponse::prepared(summary).with_preparation(
        crate::harness::DependencyPreparationTelemetry {
            status: Some(if already_available {
                crate::harness::DependencyPreparationStatus::AlreadyAvailable
            } else {
                crate::harness::DependencyPreparationStatus::Prepared
            }),
            package_count: package_count as u64,
            cache_hits: cache_hits as u64,
            packages_downloaded: package_count.saturating_sub(cache_hits) as u64,
            bytes_downloaded: bytes_downloaded as u64,
            ..Default::default()
        },
    )
}

fn create_staging_directory(snapshot_root: &Path) -> anyhow::Result<PathBuf> {
    let metadata = fs::symlink_metadata(snapshot_root)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Pip staging root is not an app-owned directory"
    );
    let stage = snapshot_root.join(format!("staging-{}", uuid::Uuid::new_v4().simple()));
    create_private_dir(&stage)?;
    Ok(stage)
}

fn create_private_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(test)]
#[path = "preparation/test_support.rs"]
mod test_support;

#[cfg(test)]
pub(in crate::harness::resource_bridge) use test_support::test_preparation_operations;

#[cfg(test)]
#[path = "preparation/tests.rs"]
mod tests;
