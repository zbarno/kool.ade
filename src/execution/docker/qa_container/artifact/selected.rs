use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path},
};

use sha2::{Digest, Sha256};

use super::QaContainerUpload;
use crate::execution::errors::ExecutionError;

/// Capture exactly the approved build artifact paths and verify each against
/// its persisted build receipt. Unlisted workspace files never enter the tar.
pub fn snapshot_selected_files(
    source_root: &Path,
    selected_paths: &[String],
    expected_hashes: &BTreeMap<String, String>,
    maximum_bytes: usize,
) -> Result<(QaContainerUpload, String), ExecutionError> {
    let denied = |message| {
        ExecutionError::new(
            crate::execution::errors::ExecutionErrorKind::PolicyDenied,
            message,
            false,
        )
    };
    if selected_paths.is_empty()
        || selected_paths.len() > 128
        || maximum_bytes == 0
        || selected_paths
            .iter()
            .any(|path| validate_path(path).is_err())
        || selected_paths.iter().collect::<BTreeSet<_>>().len() != selected_paths.len()
        || expected_hashes.len() != selected_paths.len()
        || selected_paths
            .iter()
            .any(|path| !expected_hashes.contains_key(path))
    {
        return Err(denied("Runtime QA artifact allowlist is invalid"));
    }
    let source_meta = fs::symlink_metadata(source_root)
        .map_err(|_| denied("Runtime QA artifact root cannot be inspected"))?;
    if !source_meta.is_dir() || source_meta.file_type().is_symlink() {
        return Err(denied("Runtime QA artifact root must be a real directory"));
    }
    let root = source_root
        .canonicalize()
        .map_err(|_| denied("Runtime QA artifact root cannot be resolved"))?;

    let mut files = Vec::with_capacity(selected_paths.len());
    let mut directories = BTreeSet::new();
    let mut total = 0usize;
    for relative in selected_paths {
        let path = Path::new(relative);
        let mut current = root.clone();
        let mut parents = Vec::new();
        let components = path
            .components()
            .filter_map(|component| match component {
                Component::Normal(part) => part.to_str(),
                _ => None,
            })
            .collect::<Vec<_>>();
        for part in &components[..components.len() - 1] {
            current.push(part);
            parents.push(current.strip_prefix(&root).unwrap().to_path_buf());
            let metadata = fs::symlink_metadata(&current)
                .map_err(|_| denied("Runtime QA artifact parent directory is unavailable"))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(denied(
                    "Runtime QA artifact path traverses an unsafe directory",
                ));
            }
        }
        current.push(components.last().copied().unwrap_or_default());
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| denied("Approved runtime QA artifact is missing"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(denied("Approved runtime QA artifact is not a regular file"));
        }
        let mut file = fs::File::open(&current)
            .map_err(|_| denied("Approved runtime QA artifact cannot be opened"))?;
        let opened = file
            .metadata()
            .map_err(|_| denied("Approved runtime QA artifact metadata is unavailable"))?;
        if !opened.is_file() || opened.len() != metadata.len() {
            return Err(denied(
                "Approved runtime QA artifact changed during capture",
            ));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|_| denied("Approved runtime QA artifact cannot be captured"))?;
        total = total.saturating_add(bytes.len());
        if total > maximum_bytes {
            return Err(denied(
                "Approved runtime QA artifacts exceed their byte bound",
            ));
        }
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if expected_hashes.get(relative) != Some(&digest) {
            return Err(denied(
                "Runtime QA artifact does not match its build receipt",
            ));
        }
        for parent in parents {
            directories.insert(parent);
        }
        files.push((relative.clone(), bytes, executable(&metadata)));
    }

    let mut digest = Sha256::new();
    let mut archive = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut archive);
        for directory in directories {
            let name = format!("{}/", directory.to_string_lossy().replace('\\', "/"));
            digest.update(name.as_bytes());
            digest.update([0]);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            header.set_mode(0o555);
            header.set_cksum();
            tar.append_data(&mut header, name, std::io::empty())
                .map_err(|_| denied("Runtime QA directory snapshot cannot be encoded"))?;
        }
        files.sort_by(|left, right| left.0.cmp(&right.0));
        for (path, bytes, is_executable) in files {
            digest.update(path.as_bytes());
            digest.update([0]);
            digest.update(&bytes);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_size(bytes.len() as u64);
            header.set_mode(if is_executable { 0o555 } else { 0o444 });
            header.set_cksum();
            tar.append_data(&mut header, path, bytes.as_slice())
                .map_err(|_| denied("Runtime QA artifact snapshot cannot be encoded"))?;
        }
        tar.finish()
            .map_err(|_| denied("Runtime QA artifact snapshot cannot be finalized"))?;
    }
    let upload = QaContainerUpload {
        target: "/workspace".into(),
        archive,
    };
    Ok((upload, format!("{:x}", digest.finalize())))
}

fn validate_path(value: &str) -> Result<(), ()> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 1024
        || value.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || path
            .components()
            .any(|part| matches!(part, Component::Normal(name) if name == ".git"))
    {
        return Err(());
    }
    Ok(())
}

#[cfg(unix)]
fn executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_: &fs::Metadata) -> bool {
    false
}
