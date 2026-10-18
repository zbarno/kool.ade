use std::{collections::HashSet, fs, path::Path};

mod git;
pub(super) mod hash;
use git::{tracked_executables, verify_source_revision};
use hash::{hash_file, verify_regular_file};

use super::manifest::{
    MAX_WORKSPACE_BYTES, MAX_WORKSPACE_DEPTH, MAX_WORKSPACE_ENTRIES, MAX_WORKSPACE_EXCLUSIONS,
    MAX_WORKSPACE_FILE_BYTES, WORKSPACE_MANIFEST_VERSION, WorkspaceEntryKindV1, WorkspaceEntryV1,
    WorkspaceManifestV1, is_excluded_component, root_digest, validate_relative_path,
};
use crate::execution::{
    contract::WorkspaceRef,
    errors::{ExecutionError, ExecutionErrorKind},
};

/// Revalidate a task clone's pinned source identity before reading a private
/// file into a remote transfer buffer.
pub(super) fn verify_private_file_source(
    workspace: &WorkspaceRef,
    root: &Path,
) -> anyhow::Result<()> {
    workspace.validate()?;
    let root_metadata = fs::symlink_metadata(root)?;
    anyhow::ensure!(
        root_metadata.is_dir() && !root_metadata.file_type().is_symlink(),
        "private source root is not a task clone directory"
    );
    let git_metadata = fs::symlink_metadata(root.join(".git"))?;
    anyhow::ensure!(
        git_metadata.is_dir() && !git_metadata.file_type().is_symlink(),
        "private source root has no trusted Git metadata"
    );
    verify_source_revision(root, &workspace.source_sha).map_err(anyhow::Error::msg)
}

pub(super) fn stream_file(
    path: &Path,
    expected_size: u64,
    on_chunk: impl FnMut(&[u8]) -> Result<(), ExecutionError>,
) -> Result<(String, bool), ExecutionError> {
    hash::stream_file(path, expected_size, on_chunk)
}

pub(super) fn capture(
    workspace: WorkspaceRef,
    root: &Path,
) -> Result<WorkspaceManifestV1, ExecutionError> {
    capture_snapshot(workspace, root, None)
}

pub(super) fn capture_result(
    workspace: WorkspaceRef,
    root: &Path,
    snapshot_generation: String,
) -> Result<WorkspaceManifestV1, ExecutionError> {
    capture_snapshot(workspace, root, Some(snapshot_generation))
}

fn capture_snapshot(
    workspace: WorkspaceRef,
    root: &Path,
    result_generation: Option<String>,
) -> Result<WorkspaceManifestV1, ExecutionError> {
    workspace
        .validate()
        .map_err(|_| denied("Workspace identity is invalid"))?;
    let root_meta = fs::symlink_metadata(root).map_err(|_| denied("Task clone is unavailable"))?;
    if !root_meta.is_dir() || root_meta.file_type().is_symlink() {
        return Err(denied("Task clone root is not a real directory"));
    }
    let canonical = root
        .canonicalize()
        .map_err(|_| denied("Task clone root could not be resolved"))?;
    let git_path = canonical.join(".git");
    let git_metadata = fs::symlink_metadata(&git_path)
        .map_err(|_| denied("Task clone Git metadata is unavailable"))?;
    if !git_metadata.is_dir() || git_metadata.file_type().is_symlink() {
        return Err(denied(
            "Task clone must contain a real Git metadata directory",
        ));
    }
    if result_generation.is_none() {
        verify_source_revision(&canonical, &workspace.source_sha)?;
    }
    let executable_files = tracked_executables(&canonical)?;
    let mut scan = WorkspaceScan {
        root: &canonical,
        entries: Vec::new(),
        excluded: Vec::new(),
        folded: HashSet::new(),
        total_bytes: 0,
        executable_files: &executable_files,
    };
    visit(&mut scan, &canonical, 0)?;
    scan.entries
        .sort_by(|left, right| left.path.cmp(&right.path));
    scan.excluded.sort();
    let root_sha256 = root_digest(&scan.entries, &scan.excluded)
        .map_err(|_| denied("Workspace manifest digest could not be calculated"))?;
    let manifest = WorkspaceManifestV1 {
        schema_version: WORKSPACE_MANIFEST_VERSION,
        workspace,
        snapshot_generation: result_generation
            .unwrap_or_else(|| uuid::Uuid::new_v4().hyphenated().to_string()),
        entries: scan.entries,
        excluded_paths: scan.excluded,
        total_bytes: scan.total_bytes,
        root_sha256,
    };
    manifest
        .validate()
        .map_err(|_| denied("Workspace manifest failed validation"))?;
    Ok(manifest)
}

struct WorkspaceScan<'a> {
    root: &'a Path,
    entries: Vec<WorkspaceEntryV1>,
    excluded: Vec<String>,
    folded: HashSet<String>,
    total_bytes: u64,
    executable_files: &'a HashSet<String>,
}

fn visit(
    scan: &mut WorkspaceScan<'_>,
    directory: &Path,
    depth: usize,
) -> Result<(), ExecutionError> {
    if depth > MAX_WORKSPACE_DEPTH {
        return Err(denied(
            "Task clone directory nesting exceeds the remote limit",
        ));
    }
    let directory_metadata = fs::symlink_metadata(directory)
        .map_err(|_| denied("Task clone directory changed during snapshot"))?;
    if !directory_metadata.is_dir() || directory_metadata.file_type().is_symlink() {
        return Err(denied(
            "Task clone directory changed to an unsafe file type",
        ));
    }
    let canonical_directory = directory
        .canonicalize()
        .map_err(|_| denied("Task clone directory could not be revalidated"))?;
    if !canonical_directory.starts_with(scan.root) {
        return Err(denied(
            "Task clone directory escaped its root during snapshot",
        ));
    }
    let mut children = fs::read_dir(directory)
        .map_err(|_| denied("Task clone directory could not be read"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| denied("Task clone directory changed during snapshot"))?;
    children.sort();
    for path in children {
        let relative = path
            .strip_prefix(scan.root)
            .map_err(|_| denied("Task clone path escaped its root"))?;
        let relative = path_to_manifest(relative)
            .ok_or_else(|| denied("Task clone contains a non-UTF8 path"))?;
        validate_relative_path(&relative)
            .map_err(|_| denied("Task clone contains an unsafe path"))?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| denied("Task clone changed during snapshot"))?;
        if name.eq_ignore_ascii_case(".git") {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(denied("Task clone contains invalid Git metadata"));
            }
            scan.excluded.push(relative);
            if scan.excluded.len() > MAX_WORKSPACE_EXCLUSIONS {
                return Err(denied("Task clone has too many excluded private files"));
            }
            continue;
        }
        if is_excluded_component(name) {
            scan.excluded.push(relative);
            if scan.excluded.len() > MAX_WORKSPACE_EXCLUSIONS {
                return Err(denied("Task clone has too many excluded private files"));
            }
            continue;
        }
        if !scan.folded.insert(relative.to_lowercase()) {
            return Err(denied("Task clone has case-insensitive path collisions"));
        }
        if scan.entries.len() >= MAX_WORKSPACE_ENTRIES {
            return Err(denied("Task clone has too many files for remote staging"));
        }
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            scan.entries.push(WorkspaceEntryV1 {
                path: relative,
                kind: WorkspaceEntryKindV1::Directory,
                size_bytes: 0,
                sha256: None,
                executable: false,
                symlink_target: None,
            });
            visit(scan, &path, depth + 1)?;
        } else if file_type.is_file() {
            let size_bytes = metadata.len();
            if size_bytes > MAX_WORKSPACE_FILE_BYTES
                || scan.total_bytes.saturating_add(size_bytes) > MAX_WORKSPACE_BYTES
            {
                return Err(denied("Task clone exceeds the remote workspace size limit"));
            }
            verify_regular_file(&metadata)
                .map_err(|_| denied("Task clone file metadata is unsafe"))?;
            let (sha256, executable) = hash_file(&path, size_bytes)?;
            let executable = executable || scan.executable_files.contains(&relative);
            scan.total_bytes += size_bytes;
            scan.entries.push(WorkspaceEntryV1 {
                path: relative,
                kind: WorkspaceEntryKindV1::File,
                size_bytes,
                sha256: Some(sha256),
                executable,
                symlink_target: None,
            });
        } else if file_type.is_symlink() {
            let target = fs::read_link(&path)
                .map_err(|_| denied("Task clone symlink changed during snapshot"))?;
            let target = target
                .to_str()
                .ok_or_else(|| denied("Task clone has a non-UTF8 symlink"))?
                .to_owned();
            if !super::manifest::valid_symlink_target(&relative, &target) {
                return Err(denied("Task clone symlink escapes its workspace"));
            }
            scan.entries.push(WorkspaceEntryV1 {
                path: relative,
                kind: WorkspaceEntryKindV1::Symlink,
                size_bytes: 0,
                sha256: None,
                executable: false,
                symlink_target: Some(target),
            });
        } else {
            return Err(denied(
                "Task clone contains a device or unsupported file type",
            ));
        }
    }
    Ok(())
}

fn path_to_manifest(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        let std::path::Component::Normal(part) = component else {
            return None;
        };
        parts.push(part.to_str()?);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

pub(super) fn denied(message: &'static str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::PolicyDenied, message, false)
}

#[cfg(test)]
#[path = "scan/tests.rs"]
mod tests;
