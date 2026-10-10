use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::artifacts::planning_store::{PlanningStore, StoreError};

pub(crate) fn revision(store: &PlanningStore) -> Result<String, StoreError> {
    let root_metadata = match std::fs::symlink_metadata(&store.root) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(format!("sha256:{:x}", Sha256::digest([])));
        }
        Err(source) => {
            return Err(StoreError::Io {
                path: store.root.clone(),
                source,
            });
        }
    };
    if root_metadata.file_type().is_symlink() {
        return Err(StoreError::InvalidPath(store.root.display().to_string()));
    }
    let mut files = Vec::new();
    collect_files(&store.root, &store.root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hash = Sha256::new();
    for (relative, path) in files {
        hash.update(relative.as_bytes());
        hash.update([0]);
        let bytes = std::fs::read(&path).map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    Ok(format!("sha256:{:x}", hash.finalize()))
}

fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<(), StoreError> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|source| StoreError::Io {
            path: directory.to_path_buf(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| StoreError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let relative = path.strip_prefix(root).expect("walked below root");
        if is_local_only(relative) || relative == Path::new(".git") {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(StoreError::InvalidPath(
                relative.to_string_lossy().into_owned(),
            ));
        }
        if metadata.is_dir() {
            collect_files(root, &path, files)?;
        } else if metadata.is_file() {
            files.push((relative.to_string_lossy().into_owned(), path));
        }
    }
    Ok(())
}

fn is_local_only(relative: &Path) -> bool {
    relative == Path::new("config/mcp.json")
        || relative.starts_with("implementation")
        || relative == Path::new("state/time-ledger.log")
}
