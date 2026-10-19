//! Enumerate only exact regular files already excluded from workspace staging.

#[cfg(target_os = "linux")]
use std::{ffi::CStr, io::Read, os::fd::AsRawFd, path::Path};

#[cfg(target_os = "linux")]
use crate::execution::remote::private_files::{
    MAX_PRIVATE_FILE_BYTES, MAX_PRIVATE_FILE_GRANT_FILES, PrivateDataClassV1, PrivateFileSpecV1,
    digest,
};

#[cfg(target_os = "linux")]
const MAX_DISCOVERY_ENTRIES: usize = 4096;

#[cfg(target_os = "linux")]
pub(crate) fn discover_private_files(
    workspace: &crate::execution::contract::WorkspaceRef,
    root: &Path,
    snapshot: &crate::execution::remote::WorkspaceManifestV1,
) -> anyhow::Result<Vec<PrivateFileSpecV1>> {
    snapshot.validate()?;
    anyhow::ensure!(snapshot.workspace == *workspace);
    crate::execution::remote::workspace::verify_private_file_source(workspace, root)?;
    let mut files = Vec::new();
    let mut total_bytes = 0_u64;
    let mut visited_entries = 0;
    for excluded in &snapshot.excluded_paths {
        if excluded == ".git" || excluded.starts_with(".git/") {
            continue;
        }
        walk_excluded(
            root,
            excluded,
            0,
            &mut visited_entries,
            &mut files,
            &mut total_bytes,
        )?;
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

#[cfg(target_os = "linux")]
fn walk_excluded(
    root: &Path,
    relative: &str,
    depth: usize,
    visited_entries: &mut usize,
    files: &mut Vec<PrivateFileSpecV1>,
    total_bytes: &mut u64,
) -> anyhow::Result<()> {
    anyhow::ensure!(depth <= 32, "private-file directory nesting is too deep");
    *visited_entries = (*visited_entries).saturating_add(1);
    anyhow::ensure!(
        *visited_entries <= MAX_DISCOVERY_ENTRIES,
        "excluded private directories contain too many entries"
    );
    let entry = super::open_source_file(root, relative)?;
    let metadata = entry.metadata()?;
    if metadata.is_dir() {
        let remaining = MAX_DISCOVERY_ENTRIES.saturating_sub(*visited_entries);
        for name in directory_names(entry.as_raw_fd(), remaining)? {
            if name == ".git" {
                continue;
            }
            let child = format!("{relative}/{name}");
            crate::execution::remote::private_files::validate_private_path(&child)?;
            walk_excluded(root, &child, depth + 1, visited_entries, files, total_bytes)?;
        }
        return Ok(());
    }
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "excluded private path is not a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(metadata.nlink() == 1 && metadata.mode() & 0o6000 == 0);
    }
    let size = metadata.len();
    anyhow::ensure!(size <= MAX_PRIVATE_FILE_BYTES);
    let mut bytes = zeroize::Zeroizing::new(Vec::with_capacity(size as usize));
    entry.take(size.saturating_add(1)).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 == size,
        "private file changed during scan"
    );
    *total_bytes = total_bytes.saturating_add(size);
    anyhow::ensure!(
        *total_bytes <= MAX_PRIVATE_FILE_BYTES * MAX_PRIVATE_FILE_GRANT_FILES as u64,
        "excluded private files exceed the grant size limit"
    );
    let data_class = classify(relative).ok_or_else(|| {
        anyhow::anyhow!("excluded file does not have a supported private-data class")
    })?;
    files.push(PrivateFileSpecV1 {
        path: relative.to_owned(),
        size_bytes: size,
        sha256: digest(&bytes),
        data_class,
    });
    anyhow::ensure!(files.len() <= MAX_PRIVATE_FILE_GRANT_FILES);
    Ok(())
}

#[cfg(target_os = "linux")]
fn directory_names(fd: std::os::fd::RawFd, limit: usize) -> anyhow::Result<Vec<String>> {
    let duplicate = unsafe { libc::dup(fd) };
    anyhow::ensure!(duplicate >= 0, "private directory could not be enumerated");
    let directory = unsafe { libc::fdopendir(duplicate) };
    if directory.is_null() {
        unsafe { libc::close(duplicate) };
        anyhow::bail!("private directory could not be enumerated");
    }
    let result = (|| -> anyhow::Result<Vec<String>> {
        let mut names = Vec::new();
        loop {
            let entry = unsafe { libc::readdir(directory) };
            if entry.is_null() {
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            let name = name.to_str()?;
            if name != "." && name != ".." {
                anyhow::ensure!(
                    names.len() < limit,
                    "excluded private directories contain too many entries"
                );
                names.push(name.to_owned());
            }
        }
        names.sort();
        Ok(names)
    })();
    unsafe { libc::closedir(directory) };
    result
}

#[cfg(target_os = "linux")]
fn classify(path: &str) -> Option<PrivateDataClassV1> {
    let lower = path.to_ascii_lowercase();
    let basename = lower.rsplit('/').next()?;
    if basename.ends_with(".pem")
        || basename.ends_with(".p12")
        || basename.ends_with(".pfx")
        || basename.ends_with(".key")
        || basename.ends_with(".crt")
    {
        return Some(PrivateDataClassV1::Certificate);
    }
    if lower.starts_with(".env")
        || lower.starts_with(".terraform/")
        || lower.starts_with(".azure/")
        || lower.starts_with(".docker/")
    {
        return Some(PrivateDataClassV1::ProjectConfiguration);
    }
    Some(PrivateDataClassV1::CredentialFile)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn directory_enumeration_stops_at_the_remaining_entry_budget() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("first"), b"").unwrap();
        std::fs::write(directory.path().join("second"), b"").unwrap();
        let limited = std::fs::File::open(directory.path()).unwrap();
        assert!(directory_names(limited.as_raw_fd(), 1).is_err());
        let complete = std::fs::File::open(directory.path()).unwrap();
        assert_eq!(directory_names(complete.as_raw_fd(), 2).unwrap().len(), 2);
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn discover_private_files(
    _workspace: &crate::execution::contract::WorkspaceRef,
    _root: &std::path::Path,
    _snapshot: &crate::execution::remote::WorkspaceManifestV1,
) -> anyhow::Result<Vec<crate::execution::remote::private_files::PrivateFileSpecV1>> {
    anyhow::bail!("private-file discovery requires Linux handle-relative reads")
}
