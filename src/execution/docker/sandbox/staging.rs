//! Bounded task-clone staging and optimistic change synchronization.
mod archive;
mod artifacts;
mod reviewed;
mod sync;

use super::ValidatedWorkspace;
use crate::execution::errors::{ExecutionError, ExecutionErrorKind};
use std::path::PathBuf;

pub(crate) const MAX_STAGED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
pub(crate) use reviewed::capture_reviewed;
#[cfg(test)]
pub(crate) use reviewed::verify_snapshot_matches_git_tree;

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceSnapshot {
    pub(crate) archive: Vec<u8>,
    pub(crate) manifest: archive::Manifest,
    pub(crate) scope_root: Option<PathBuf>,
}

impl WorkspaceSnapshot {
    pub(crate) fn record_entries(
        &self,
    ) -> Vec<crate::persistence::execution_records::ExecutionWorkspaceEntryV1> {
        self.manifest
            .iter()
            .map(|(path, fingerprint)| {
                use crate::persistence::execution_records::{
                    ExecutionWorkspaceEntryKindV1 as Kind, ExecutionWorkspaceEntryV1 as Entry,
                };
                let (kind, sha256, size_bytes, mode) = match fingerprint {
                    archive::Fingerprint::Directory => (Kind::Directory, None, 0, 0o755),
                    archive::Fingerprint::File { sha256, size, mode } => {
                        (Kind::File, Some(sha256.clone()), *size, *mode)
                    }
                };
                Entry {
                    relative_path: path.to_string_lossy().into_owned(),
                    kind,
                    sha256,
                    size_bytes,
                    mode,
                }
            })
            .collect()
    }

    pub(crate) fn from_record_entries(
        entries: &[crate::persistence::execution_records::ExecutionWorkspaceEntryV1],
        scope_root: Option<PathBuf>,
    ) -> Result<Self, ExecutionError> {
        use crate::persistence::execution_records::ExecutionWorkspaceEntryKindV1 as Kind;
        let mut manifest = archive::Manifest::new();
        if scope_root.as_ref().is_some_and(|scope| {
            scope.as_os_str().is_empty()
                || scope.is_absolute()
                || (scope != std::path::Path::new(".")
                    && scope
                        .components()
                        .any(|part| !matches!(part, std::path::Component::Normal(_))))
        }) {
            return Err(failure("Persisted scoped workspace root is invalid"));
        }
        for entry in entries {
            let path = PathBuf::from(&entry.relative_path);
            archive::validate_relative(&path)
                .map_err(|_| failure("Persisted workspace baseline is invalid"))?;
            let scoped_ancestor = scope_root.as_ref().is_some_and(|scope| {
                scope != std::path::Path::new(".") && scope.starts_with(&path)
            });
            if scope_root.as_ref().is_some_and(|scope| {
                scope != std::path::Path::new(".") && !path.starts_with(scope) && !scoped_ancestor
            }) {
                return Err(failure(
                    "Persisted workspace baseline escapes its scoped write grant",
                ));
            }
            if path == std::path::Path::new(".git") || path.starts_with(".git") {
                return Err(failure(
                    "Persisted workspace baseline includes Git metadata",
                ));
            }
            let fingerprint = match entry.kind {
                Kind::Directory => archive::Fingerprint::Directory,
                Kind::File => archive::Fingerprint::File {
                    sha256: entry
                        .sha256
                        .clone()
                        .ok_or_else(|| failure("Persisted workspace baseline is incomplete"))?,
                    size: entry.size_bytes,
                    mode: entry.mode,
                },
            };
            if scoped_ancestor && fingerprint != archive::Fingerprint::Directory {
                return Err(failure(
                    "Persisted scoped workspace ancestor is not a directory",
                ));
            }
            if manifest.insert(path, fingerprint).is_some() {
                return Err(failure(
                    "Persisted workspace baseline contains duplicate paths",
                ));
            }
        }
        Ok(Self {
            archive: Vec::new(),
            manifest,
            scope_root,
        })
    }
}

pub(crate) fn capture(
    workspace: &ValidatedWorkspace,
    budget: u64,
    read_only: bool,
) -> Result<WorkspaceSnapshot, ExecutionError> {
    workspace.revalidate()?;
    let limit = budget.min(MAX_STAGED_BYTES);
    archive::capture(workspace.root(), limit, read_only, MAX_ENTRIES)
        .map(|mut snapshot| {
            snapshot.scope_root = None;
            snapshot
        })
        .map_err(|_| failure("Workspace cannot be staged safely within its size limit"))
}

pub(crate) fn capture_scoped(
    workspace: &ValidatedWorkspace,
    root_relative: &str,
    budget: u64,
) -> Result<WorkspaceSnapshot, ExecutionError> {
    workspace.revalidate()?;
    let scope = checked_scope(workspace.root(), root_relative)?;
    let limit = budget.min(MAX_STAGED_BYTES);
    let mut snapshot = archive::capture(&scope, limit, false, MAX_ENTRIES).map_err(|error| {
        failure(&format!(
            "Scoped workspace cannot be staged safely: {}",
            scoped_capture_failure_reason(&error)
        ))
    })?;
    let prefix = if root_relative == "." {
        PathBuf::new()
    } else {
        PathBuf::from(root_relative)
    };
    snapshot.manifest = snapshot
        .manifest
        .into_iter()
        .map(|(path, fingerprint)| (prefix.join(path), fingerprint))
        .collect();
    let mut ancestor = PathBuf::new();
    let components = prefix.components().collect::<Vec<_>>();
    for component in components {
        ancestor.push(component);
        snapshot
            .manifest
            .insert(ancestor.clone(), archive::Fingerprint::Directory);
    }
    if snapshot.manifest.len() > MAX_ENTRIES {
        return Err(failure(
            "Scoped workspace has too many entries to stage safely",
        ));
    }
    snapshot.scope_root = Some(PathBuf::from(root_relative));
    Ok(snapshot)
}

fn scoped_capture_failure_reason(error: &anyhow::Error) -> &'static str {
    let reason = error.to_string();
    let io_kind = error.chain().find_map(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .map(std::io::Error::kind)
    });
    if reason.contains("hard link or special file") {
        "it contains a hard link"
    } else if reason.contains("special file") {
        "it contains a special filesystem entry"
    } else if reason.contains("symlink") {
        "it contains a symbolic link"
    } else if reason.contains("changed while staging") {
        "the workspace changed while being captured"
    } else if reason.contains("too many entries") {
        "the workspace entry limit was exceeded"
    } else if reason.contains("PermissionDenied")
        || io_kind == Some(std::io::ErrorKind::PermissionDenied)
    {
        "a filesystem entry is not accessible to the controller"
    } else if reason.contains("NotFound") || io_kind == Some(std::io::ErrorKind::NotFound) {
        "a filesystem entry disappeared while being captured"
    } else if reason.contains("exceeds staging budget")
        || reason.contains("exceeds its staging budget")
    {
        "the workspace exceeds its byte limit"
    } else {
        "a filesystem safety check failed"
    }
}

pub(crate) fn synchronize(
    workspace: &ValidatedWorkspace,
    snapshot: &WorkspaceSnapshot,
    returned_archive: &[u8],
    budget: u64,
) -> Result<(), ExecutionError> {
    synchronize_with_artifacts(workspace, snapshot, returned_archive, budget, &[]).map(|_| ())
}

pub(crate) fn synchronize_with_artifacts(
    workspace: &ValidatedWorkspace,
    snapshot: &WorkspaceSnapshot,
    returned_archive: &[u8],
    budget: u64,
    artifact_paths: &[String],
) -> Result<Vec<crate::execution::contract::ArtifactManifestEntryV1>, ExecutionError> {
    workspace.revalidate()?;
    let limit = budget.min(MAX_STAGED_BYTES);
    let parsed = archive::parse_returned(returned_archive, limit, MAX_ENTRIES)
        .map_err(|_| failure("Container returned an invalid or oversized workspace archive"))?;
    let artifacts = collect_parsed_artifacts(&parsed, artifact_paths, limit)?;
    let entries = if let Some(scope) = &snapshot.scope_root
        && scope != std::path::Path::new(".")
    {
        parsed
            .into_iter()
            .map(|(path, entry)| (scope.join(path), entry))
            .collect()
    } else {
        parsed
    };
    sync::apply_entries(workspace.root(), &snapshot.manifest, entries, limit)
        .map_err(|error| failure(&error))?;
    Ok(artifacts)
}

pub(crate) fn collect_artifacts(
    returned_archive: &[u8],
    budget: u64,
    artifact_paths: &[String],
) -> Result<Vec<crate::execution::contract::ArtifactManifestEntryV1>, ExecutionError> {
    let limit = budget.min(MAX_STAGED_BYTES);
    let parsed = archive::parse_returned(returned_archive, limit, MAX_ENTRIES)
        .map_err(|_| failure("Container returned an invalid or oversized workspace archive"))?;
    collect_parsed_artifacts(&parsed, artifact_paths, limit)
}

fn collect_parsed_artifacts(
    parsed: &std::collections::BTreeMap<PathBuf, archive::ReturnedEntry>,
    artifact_paths: &[String],
    limit: u64,
) -> Result<Vec<crate::execution::contract::ArtifactManifestEntryV1>, ExecutionError> {
    artifacts::collect(parsed, artifact_paths, limit)
        .map_err(|_| failure("Container did not return the requested bounded artifacts"))
}

fn checked_scope(root: &std::path::Path, relative: &str) -> Result<PathBuf, ExecutionError> {
    if relative == "." {
        return root
            .canonicalize()
            .map_err(|_| failure("Scoped workspace directory is unavailable"));
    }
    let path = PathBuf::from(relative);
    if relative.is_empty()
        || relative.contains('\\')
        || path.components().any(|part| {
            !matches!(part, std::path::Component::Normal(_)) || part.as_os_str() == ".git"
        })
    {
        return Err(failure("Scoped workspace path is invalid"));
    }
    let joined = root.join(&path);
    let mut cursor = root.to_path_buf();
    for component in path.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(failure("Scoped workspace path is invalid"));
        };
        cursor.push(name);
        let metadata = std::fs::symlink_metadata(&cursor)
            .map_err(|_| failure("Scoped workspace directory is unavailable"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(failure(
                "Scoped workspace path contains a symlink or non-directory",
            ));
        }
    }
    let canonical = joined
        .canonicalize()
        .map_err(|_| failure("Scoped workspace directory is unavailable"))?;
    if !canonical.starts_with(root) || !canonical.is_dir() {
        return Err(failure("Scoped workspace directory escapes the task clone"));
    }
    Ok(canonical)
}

fn failure(message: &str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::ExecutionFailed, message, false)
}

#[cfg(test)]
#[path = "staging/tests.rs"]
mod tests;
