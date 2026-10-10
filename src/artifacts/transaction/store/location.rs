use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::artifacts::planning_store::{PlanningStore, StoreError};

use super::{JOURNAL, LOCK};

pub(super) fn remove_journal(path: &Path) -> Result<(), StoreError> {
    std::fs::remove_file(path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    crate::artifacts::sync_parent_directory(path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn transaction_root(_store: &PlanningStore) -> Result<PathBuf, StoreError> {
    // Transaction journals are operator-local recovery state, not planning
    // records. Keep their location stable whether the store is in a Git repo
    // or a plain directory so restarts never depend on Git availability.
    let root = crate::persistence::state_root().join("planning-transactions");
    std::fs::create_dir_all(&root).map_err(|source| StoreError::Io {
        path: root.clone(),
        source,
    })?;
    Ok(root)
}

pub(super) fn store_identity(store: &PlanningStore) -> String {
    std::fs::canonicalize(&store.root)
        .unwrap_or_else(|_| store.root.clone())
        .to_string_lossy()
        .into_owned()
}

pub(super) fn journal_path(common: &Path, root: &str) -> PathBuf {
    common.join(format!(
        "{JOURNAL}-{:x}.json",
        Sha256::digest(root.as_bytes())
    ))
}

pub(super) fn acquire_lock(common: &Path, root: &str) -> Result<std::fs::File, StoreError> {
    let lock_path = common.join(format!("{LOCK}-{:x}.lock", Sha256::digest(root.as_bytes())));
    match std::fs::symlink_metadata(&lock_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(StoreError::InvalidPath(lock_path.display().to_string()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(StoreError::Io {
                path: lock_path,
                source,
            });
        }
    }
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|source| StoreError::Io {
            path: lock_path.clone(),
            source,
        })?;
    lock.lock().map_err(|source| StoreError::Io {
        path: lock_path,
        source,
    })?;
    Ok(lock)
}
