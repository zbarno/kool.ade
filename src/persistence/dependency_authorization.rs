//! User-local, exact-package dependency grants. No trust choice is written to Git.
use crate::harness::{DependencyAuthorizationScope, DependencyNeed, DependencyRequest};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

mod identity;
pub(crate) use identity::project_id;

const MAX_GRANTS: usize = 512;
const MAX_FILE_BYTES: u64 = 256 * 1024;
const SCHEMA_VERSION: u8 = 2;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Store {
    schema_version: u8,
    project_id: String,
    grants: Vec<Grant>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyStoreV1 {
    schema_version: u8,
    project_root: PathBuf,
    grants: Vec<Grant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Grant {
    task_id: String,
    need: DependencyNeed,
    scope: DependencyAuthorizationScope,
}

pub fn save(
    root: &Path,
    request: &DependencyRequest,
    scope: DependencyAuthorizationScope,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        scope == DependencyAuthorizationScope::Project,
        "One-time dependency grants are kept in memory and cannot be persisted"
    );
    let project_id = project_id(root)?;
    let path = store_path(&project_id)?;
    // Guard the entire read/modify/atomic-replace transaction. Atomic rename
    // alone cannot prevent concurrent windows from losing each other's grants.
    let _lock = acquire_store_lock(&path)?;
    let mut store = load(&project_id, &path)?;
    let grant = Grant {
        task_id: request.task_id.clone(),
        need: request.need.clone(),
        scope,
    };
    if let Some(existing) = store.grants.iter_mut().find(|existing| {
        same_need(&existing.need, &grant.need)
            && existing.task_id == grant.task_id
            && existing.scope == grant.scope
    }) {
        *existing = grant;
    } else {
        anyhow::ensure!(
            store.grants.len() < MAX_GRANTS,
            "Dependency authorization store reached its grant limit"
        );
        store.grants.push(grant);
    }
    let bytes = serde_json::to_vec_pretty(&store)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_FILE_BYTES,
        "Dependency authorization store is too large"
    );
    crate::artifacts::atomic_write_bytes(&path, &bytes)?;
    set_private_file(&path)?;
    Ok(())
}

/// A lock file is distinct from the atomically replaced JSON store. Locking
/// the JSON inode itself would not protect writers after an atomic rename.
fn acquire_store_lock(store: &Path) -> anyhow::Result<File> {
    let path = store.with_extension("lock");
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Dependency authorization lock must be a regular file"
            );
            check_private_file(&path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    // The containing user-owned directory is created and permission-restricted
    // by store_path, so other accounts cannot manipulate the lock entry.
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    let metadata = fs::symlink_metadata(&path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Dependency authorization lock must not be a symlink"
    );
    set_private_file(&path)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(std::fs::TryLockError::WouldBlock) => {
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "Another Kool.ad/e instance is updating dependency permissions; retry shortly"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

pub fn matching_scope(
    root: &Path,
    _task_id: &str,
    need: &DependencyNeed,
) -> anyhow::Result<Option<DependencyAuthorizationScope>> {
    let Ok(project_id) = project_id(root) else {
        return Ok(None);
    };
    let path = store_path(&project_id)?;
    let store = load(&project_id, &path)?;
    Ok(store.grants.into_iter().find_map(|grant| {
        (grant.scope == DependencyAuthorizationScope::Project && same_need(&grant.need, need))
            .then_some(grant.scope)
    }))
}

fn store_path(project_id: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        !project_id.is_empty()
            && project_id.len() <= 128
            && project_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
        "Invalid dependency authorization project identity"
    );
    let state_root = crate::persistence::state_root();
    fs::create_dir_all(&state_root)?;
    let state_root = state_root.canonicalize()?;
    let projects = state_root.join("projects");
    ensure_private_dir(&projects)?;
    let path = crate::persistence::project_dir(project_id).join("dependency-authorizations.json");
    ensure_private_dir(path.parent().unwrap())?;
    anyhow::ensure!(
        path.parent()
            .unwrap()
            .canonicalize()?
            .starts_with(&state_root),
        "Dependency authorization path escapes the user state directory"
    );
    Ok(path)
}

fn load(project_id: &str, path: &Path) -> anyhow::Result<Store> {
    let store = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Store {
            schema_version: SCHEMA_VERSION,
            project_id: project_id.to_owned(),
            grants: Vec::new(),
        },
        Err(error) => return Err(error.into()),
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Dependency authorization store must be a regular file"
            );
            anyhow::ensure!(
                metadata.len() <= MAX_FILE_BYTES,
                "Dependency authorization store is too large"
            );
            check_private_file(path)?;
            let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
            match value
                .get("schemaVersion")
                .and_then(serde_json::Value::as_u64)
            {
                Some(1) => {
                    let legacy: LegacyStoreV1 = serde_json::from_value(value)?;
                    anyhow::ensure!(
                        legacy.schema_version == 1
                            && crate::persistence::project_slug(&legacy.project_root) == project_id,
                        "Legacy dependency authorization store does not match this project"
                    );
                    Store {
                        schema_version: SCHEMA_VERSION,
                        project_id: project_id.to_owned(),
                        grants: legacy.grants,
                    }
                }
                Some(version) if version == u64::from(SCHEMA_VERSION) => {
                    serde_json::from_value(value)?
                }
                _ => anyhow::bail!("Unsupported dependency authorization store version"),
            }
        }
    };
    anyhow::ensure!(
        store.schema_version == SCHEMA_VERSION
            && store.project_id == project_id
            && store.grants.len() <= MAX_GRANTS,
        "Dependency authorization store does not match this project"
    );
    Ok(store)
}

fn same_need(left: &DependencyNeed, right: &DependencyNeed) -> bool {
    left.ecosystem == right.ecosystem
        && left.package == right.package
        && left.version == right.version
        && left.source == right.source
        && left.kind == right.kind
        && left.command == right.command
        && left.lockfile_identity == right.lockfile_identity
        && left.introduced_packages == right.introduced_packages
}

#[cfg(test)]
mod tests;

#[cfg(unix)]
fn ensure_private_dir(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    match fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Dependency authorization directory contains a symlink or non-directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Another process may create the same directory after the check.
            // Treat EEXIST as contention, then validate the actual directory
            // rather than trusting the concurrently created path.
            if let Err(error) = fs::create_dir(path)
                && error.kind() != std::io::ErrorKind::AlreadyExists
            {
                return Err(error.into());
            }
            let metadata = fs::symlink_metadata(path)?;
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Dependency authorization directory contains a symlink or non-directory"
            );
        },
        Err(error) => return Err(error.into()),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file(_: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn check_private_file(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    anyhow::ensure!(
        fs::metadata(path)?.permissions().mode() & 0o077 == 0,
        "Dependency authorization store must be private to the current user"
    );
    Ok(())
}

#[cfg(not(unix))]
fn check_private_file(_: &Path) -> anyhow::Result<()> {
    Ok(())
}
