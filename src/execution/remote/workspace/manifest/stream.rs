use super::{WorkspaceEntryKindV1, WorkspaceManifestV1};
use crate::execution::errors::ExecutionError;
use std::path::Path;

pub(super) fn stream_file(
    manifest: &WorkspaceManifestV1,
    root: &Path,
    relative_path: &str,
    on_chunk: impl FnMut(&[u8]) -> Result<(), ExecutionError>,
) -> Result<(), ExecutionError> {
    manifest
        .validate()
        .map_err(|_| super::super::scan::denied("Workspace manifest is invalid"))?;
    let entry = manifest
        .entries
        .iter()
        .find(|entry| entry.path == relative_path && entry.kind == WorkspaceEntryKindV1::File)
        .ok_or_else(|| super::super::scan::denied("Workspace file is not in the manifest"))?;
    let root_metadata = std::fs::symlink_metadata(root)
        .map_err(|_| super::super::scan::denied("Task clone root is unavailable"))?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(super::super::scan::denied(
            "Task clone root is not a real directory",
        ));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|_| super::super::scan::denied("Task clone root could not be resolved"))?;
    let mut path = canonical_root;
    let components = relative_path.split('/').collect::<Vec<_>>();
    for component in &components[..components.len().saturating_sub(1)] {
        path.push(component);
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|_| super::super::scan::denied("Workspace directory changed after capture"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(super::super::scan::denied(
                "Workspace path changed to an unsafe directory",
            ));
        }
    }
    path.push(components[components.len() - 1]);
    let expected_hash = entry
        .sha256
        .as_deref()
        .ok_or_else(|| super::super::scan::denied("Workspace file digest is missing"))?;
    let (actual_hash, _) = super::super::scan::stream_file(&path, entry.size_bytes, on_chunk)?;
    if actual_hash != expected_hash {
        return Err(super::super::scan::denied(
            "Workspace file changed after snapshot capture",
        ));
    }
    Ok(())
}
