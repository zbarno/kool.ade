//! Bounded task-clone staging and optimistic change synchronization.
mod archive;
mod sync;

use super::ValidatedWorkspace;
use crate::execution::errors::{ExecutionError, ExecutionErrorKind};
use std::path::PathBuf;

pub(crate) const MAX_STAGED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;

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
                || scope
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
        }) {
            return Err(failure("Persisted scoped workspace root is invalid"));
        }
        for entry in entries {
            let path = PathBuf::from(&entry.relative_path);
            archive::validate_relative(&path)
                .map_err(|_| failure("Persisted workspace baseline is invalid"))?;
            if scope_root
                .as_ref()
                .is_some_and(|scope| !path.starts_with(scope))
            {
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
    let mut snapshot = archive::capture(&scope, limit, false, MAX_ENTRIES)
        .map_err(|_| failure("Scoped workspace cannot be staged safely within its size limit"))?;
    let prefix = PathBuf::from(root_relative);
    snapshot.manifest = snapshot
        .manifest
        .into_iter()
        .map(|(path, fingerprint)| (prefix.join(path), fingerprint))
        .collect();
    snapshot.scope_root = Some(prefix);
    Ok(snapshot)
}

pub(crate) fn synchronize(
    workspace: &ValidatedWorkspace,
    snapshot: &WorkspaceSnapshot,
    returned_archive: &[u8],
    budget: u64,
) -> Result<(), ExecutionError> {
    workspace.revalidate()?;
    let returned = returned_archive;
    let result = if let Some(scope) = &snapshot.scope_root {
        let parsed = archive::parse_returned(returned, budget.min(MAX_STAGED_BYTES), MAX_ENTRIES)
            .map_err(|_| {
            failure("Container returned an invalid or oversized scoped workspace archive")
        })?;
        let scoped = parsed
            .into_iter()
            .map(|(path, entry)| (scope.join(path), entry))
            .collect();
        sync::apply_entries(
            workspace.root(),
            &snapshot.manifest,
            scoped,
            budget.min(MAX_STAGED_BYTES),
        )
    } else {
        sync::apply(
            workspace.root(),
            &snapshot.manifest,
            returned,
            budget.min(MAX_STAGED_BYTES),
            MAX_ENTRIES,
        )
    };
    result.map_err(|error| failure(&error))
}

fn checked_scope(root: &std::path::Path, relative: &str) -> Result<PathBuf, ExecutionError> {
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
