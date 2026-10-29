use super::{cache, lockfile};
use crate::harness::resource_bridge::ResourceResponse;
use std::{
    fs,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicUsize},
};

mod host;
mod telemetry;
use telemetry::prepared_response;

pub(crate) struct SharedPreparationOperations {
    fetch: Arc<GoFetchOperation>,
    verify_existing: Arc<GoVerifyOperation>,
}

type GoFetchOperation = dyn Fn(&Path, &Path, usize) -> (usize, anyhow::Result<()>) + Send + Sync;
type GoVerifyOperation = dyn Fn(&Path, &Path, &str) -> anyhow::Result<()> + Send + Sync;

#[cfg(test)]
pub(super) fn test_preparation_operations(
    fetch: impl Fn(&Path, &Path, usize) -> (usize, anyhow::Result<()>) + Send + Sync + 'static,
) -> Arc<SharedPreparationOperations> {
    test_preparation_operations_with_verifier(fetch, |_, _, _| Ok(()))
}

#[cfg(test)]
fn test_preparation_operations_with_verifier(
    fetch: impl Fn(&Path, &Path, usize) -> (usize, anyhow::Result<()>) + Send + Sync + 'static,
    verify_existing: impl Fn(&Path, &Path, &str) -> anyhow::Result<()> + Send + Sync + 'static,
) -> Arc<SharedPreparationOperations> {
    Arc::new(SharedPreparationOperations {
        fetch: Arc::new(fetch),
        verify_existing: Arc::new(verify_existing),
    })
}

pub(super) fn prepare_with_download_policy(
    worktree: &Path,
    cache_root: &Path,
    snapshot_staging: &Path,
    downloaded_bytes: &AtomicUsize,
    allow_downloads: bool,
    operations: Option<&SharedPreparationOperations>,
    expected_identity: &str,
) -> anyhow::Result<ResourceResponse> {
    let packages = lockfile::collect(worktree)?;
    anyhow::ensure!(
        lockfile::identity(&packages)? == expected_identity,
        "The authorized go.mod/go.sum identity changed before package preparation"
    );
    anyhow::ensure!(
        !worktree.join("vendor").try_exists()?,
        "Go vendor directories are unsupported for brokered module restores"
    );
    let existing = cache::snapshot_root(cache_root, expected_identity)?;
    let existing_is_verified = existing.join("pkg/mod").is_dir()
        && validate_cache_tree(&existing).is_ok()
        && match operations {
            Some(operations) => {
                (operations.verify_existing)(worktree, &existing, expected_identity).is_ok()
            }
            None => host::verify_existing_cache(worktree, &existing, expected_identity).is_ok(),
        };
    let task_identity_is_current =
        lockfile::identity(&lockfile::collect(worktree)?)? == expected_identity;
    if existing_is_verified && task_identity_is_current {
        let package_count = packages.packages.len();
        return Ok(prepared_response(package_count, package_count, 0, true));
    }
    if !allow_downloads {
        return Ok(ResourceResponse::needs_attention(
            "Fresh Go module downloads are disabled while private project configuration is mounted; the exact go.sum modules are not already verified in the app-owned cache.".into(),
        ));
    }

    let (workspace, gopath) = create_stage(snapshot_staging)?;
    let stage_result = fetch_stage(
        worktree,
        &workspace,
        &gopath,
        downloaded_bytes,
        expected_identity,
        operations,
    );
    let (cache_hits, bytes_downloaded) = match stage_result {
        Ok(outcome) => outcome,
        Err(error) => {
            let _ = fs::remove_dir_all(workspace.parent().unwrap_or(&workspace));
            return Err(error);
        }
    };
    anyhow::ensure!(
        lockfile::identity(&lockfile::collect(&workspace)?)? == expected_identity,
        "Go modified go.mod or go.sum during module preparation; unlocked checksums are not accepted"
    );
    anyhow::ensure!(
        lockfile::identity(&lockfile::collect(worktree)?)? == expected_identity,
        "Go task dependency inputs changed during package preparation"
    );
    if operations.is_none() {
        host::verify_offline(worktree, &workspace, &gopath)?;
    }
    validate_cache_tree(&gopath)?;
    publish_stage(&gopath, &existing)?;
    let _ = fs::remove_dir_all(workspace.parent().unwrap_or(&workspace));
    Ok(prepared_response(
        packages.packages.len(),
        cache_hits,
        bytes_downloaded,
        false,
    ))
}

fn fetch_stage(
    worktree: &Path,
    workspace: &Path,
    gopath: &Path,
    downloaded_bytes: &AtomicUsize,
    expected_identity: &str,
    operations: Option<&SharedPreparationOperations>,
) -> anyhow::Result<(usize, usize)> {
    copy_lockfile(worktree, workspace, "go.mod")?;
    copy_lockfile(worktree, workspace, "go.sum")?;
    anyhow::ensure!(
        lockfile::identity(&lockfile::collect(workspace)?)? == expected_identity,
        "Go task dependency inputs changed while preparing the broker workspace"
    );
    let reservation = crate::harness::resource_bridge::budget::reserve_downloads(
        downloaded_bytes,
        crate::harness::resource_bridge::MAX_SESSION_BYTES,
        crate::harness::resource_bridge::MAX_SESSION_BYTES,
    )?;
    let (bytes_received, result) = match operations {
        Some(operations) => (operations.fetch)(workspace, gopath, reservation),
        None => {
            let outcome = host::fetch_from_proxy(worktree, workspace, gopath, reservation);
            (outcome.bytes_received, outcome.result)
        }
    };
    crate::harness::resource_bridge::budget::settle_downloads(
        downloaded_bytes,
        reservation,
        bytes_received,
    );
    let bytes_downloaded = bytes_received;
    result?;
    Ok((0, bytes_downloaded))
}

fn publish_stage(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Go snapshot has no parent directory"))?;
    let staged = parent.join(format!(
        ".go-cache-staging-{}",
        uuid::Uuid::new_v4().simple()
    ));
    copy_tree(source, &staged)?;
    validate_cache_tree(&staged)?;
    let backup = parent.join(format!(
        ".go-cache-invalid-{}",
        uuid::Uuid::new_v4().simple()
    ));
    if let Ok(metadata) = fs::symlink_metadata(destination) {
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Go module snapshot destination is not an app-owned directory"
        );
        fs::rename(destination, &backup)?;
    }
    if let Err(error) = fs::rename(&staged, destination) {
        if backup.exists() {
            let _ = fs::rename(&backup, destination);
        }
        let _ = fs::remove_dir_all(staged);
        return Err(error.into());
    }
    if backup.exists() {
        fs::remove_dir_all(backup)?;
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> anyhow::Result<()> {
    fs::create_dir(destination)?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700))?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() && !kind.is_symlink() {
            copy_tree(&entry.path(), &target)?;
        } else {
            anyhow::ensure!(
                kind.is_file() && !kind.is_symlink(),
                "Go module cache contains a link or special file"
            );
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn validate_cache_tree(root: &Path) -> anyhow::Result<()> {
    let mut pending = vec![root.to_owned()];
    let mut total_bytes = 0_u64;
    while let Some(directory) = pending.pop() {
        let metadata = fs::symlink_metadata(&directory)?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Go module cache contains a linked directory"
        );
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            anyhow::ensure!(!kind.is_symlink(), "Go module cache contains a symlink");
            if kind.is_dir() {
                pending.push(entry.path());
            } else {
                anyhow::ensure!(kind.is_file(), "Go module cache contains a special file");
                total_bytes = total_bytes.saturating_add(entry.metadata()?.len());
                anyhow::ensure!(
                    total_bytes <= crate::harness::resource_bridge::MAX_CACHE_SNAPSHOT_BYTES,
                    "Go module cache exceeds the snapshot limit"
                );
            }
        }
    }
    Ok(())
}

fn create_stage(snapshot_staging: &Path) -> anyhow::Result<(PathBuf, PathBuf)> {
    let metadata = fs::symlink_metadata(snapshot_staging)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Go staging root is not an app-owned directory"
    );
    let root = snapshot_staging.join(format!("go-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    let workspace = root.join("workspace");
    fs::create_dir(&workspace)?;
    fs::set_permissions(&workspace, fs::Permissions::from_mode(0o700))?;
    let gopath = root.join("gopath");
    fs::create_dir(&gopath)?;
    fs::set_permissions(&gopath, fs::Permissions::from_mode(0o700))?;
    Ok((workspace, gopath))
}

fn copy_lockfile(source: &Path, destination: &Path, name: &str) -> anyhow::Result<()> {
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let source_file = options.open(source.join(name))?;
    let metadata = source_file.metadata()?;
    let limit = if name == "go.mod" {
        1024 * 1024
    } else {
        8 * 1024 * 1024
    };
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= limit,
        "Go lock input is not a bounded regular file"
    );
    let mut input = Vec::with_capacity(metadata.len() as usize);
    use std::io::Read as _;
    source_file.take(limit + 1).read_to_end(&mut input)?;
    anyhow::ensure!(
        input.len() as u64 <= limit,
        "Go lock input grew beyond its scan limit"
    );
    let target = destination.join(name);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(target)?;
    use std::io::Write;
    file.write_all(&input)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
#[path = "prepare/tests.rs"]
mod tests;
