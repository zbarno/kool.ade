//! Windows stage-out uses handle-relative opens and no-replace renames. Replacing
//! an existing file briefly removes its name while the staged file is installed;
//! if installation fails, the prior file is restored or kept under a unique
//! `.koolade-conflict-*` name rather than overwritten. File contents are flushed
//! before rename; Windows does not expose a portable directory flush operation.
//! Read-only cleanup uses `FileDispositionInfoEx` (Windows 10 version 1709+);
//! unsupported filesystems fail closed and retain the displaced file.
//! Git-tracked executable bits are persisted with `update-index --chmod` and
//! rolled back with a failed file transaction. New executable outputs are
//! rejected. Windows applies only the DOS read-only attribute to files; Unix
//! execute, group, and other permission bits have no filesystem representation.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use super::super::super::workspace_identity::WorkspaceObjectId;
use super::super::archive::{Fingerprint, Manifest, ReturnedEntry, validate_relative};

mod fs;
mod index;
mod native;
mod write;
use fs::{create_directory, fingerprint_at, open_root_nofollow, pin_git_metadata};
use index::IndexModes;
use write::{WriteOptions, remove_entry, write_file};

pub(super) fn apply(
    root: &Path,
    expected_root_identity: &WorkspaceObjectId,
    expected_git_identity: &WorkspaceObjectId,
    baseline: &Manifest,
    returned: &BTreeMap<PathBuf, ReturnedEntry>,
    byte_limit: u64,
    source_output_paths: Option<&[String]>,
) -> Result<(), String> {
    apply_with_hook(
        root,
        expected_root_identity,
        expected_git_identity,
        baseline,
        returned,
        byte_limit,
        source_output_paths,
        |_| false,
    )
}

// Keep the full transaction request and its race-test hook explicit at this boundary.
#[allow(clippy::too_many_arguments)]
fn apply_with_hook(
    root_path: &Path,
    expected_root_identity: &WorkspaceObjectId,
    expected_git_identity: &WorkspaceObjectId,
    baseline: &Manifest,
    returned: &BTreeMap<PathBuf, ReturnedEntry>,
    byte_limit: u64,
    source_output_paths: Option<&[String]>,
    mut hook: impl FnMut(Checkpoint) -> bool,
) -> Result<(), String> {
    let root = open_root_nofollow(root_path, expected_root_identity)?;
    let changed = changed_entries(baseline, returned);
    if changed.is_empty() {
        return Ok(());
    }
    let _git_metadata = pin_git_metadata(&root, expected_git_identity)?;
    if let Some(allowed_outputs) = source_output_paths {
        for output in allowed_outputs {
            if output != "." {
                validate_relative(Path::new(output))
                    .map_err(|_| "Approved recipe output path is unsafe")?;
            }
        }
        if changed.iter().any(|path| {
            !allowed_outputs
                .iter()
                .any(|allowed| output_path_covers(allowed, path, baseline, returned))
        }) {
            return Err("Container changed a path outside the approved recipe outputs".into());
        }
    }
    let index_modes = IndexModes::read(root_path)?;
    for (path, entry) in returned {
        if matches!(entry.fingerprint, Fingerprint::File { .. })
            && entry.mode & 0o111 != 0
            && index_modes.executable(path)?.is_none()
        {
            return Err(
                "New executable workspace outputs are not synchronized safely on Windows".into(),
            );
        }
    }

    let mut pending = BTreeSet::new();
    for path in &changed {
        let index_executable = index_modes.executable(path)?;
        let current = fingerprint_at(&root, path, byte_limit, index_executable)?;
        if current.as_ref() == baseline.get(path) {
            pending.insert(path.clone());
        } else if !matches_returned(current.as_ref(), returned.get(path)) {
            return Err("Task clone changed while the container was running; container changes were not synchronized".into());
        }
        if let (Some(original), Some(update)) = (baseline.get(path), returned.get(path)) {
            let same_kind = matches!(
                (original, &update.fingerprint),
                (Fingerprint::Directory, Fingerprint::Directory)
                    | (Fingerprint::File { .. }, Fingerprint::File { .. })
            );
            if !same_kind {
                return Err(
                    "Workspace file and directory type changes are not synchronized safely".into(),
                );
            }
        }
    }
    if pending.is_empty() {
        return Ok(());
    }

    let mut directories = pending
        .iter()
        .filter(|path| {
            returned
                .get(*path)
                .is_some_and(|entry| entry.fingerprint == Fingerprint::Directory)
                && !baseline.contains_key(*path)
        })
        .cloned()
        .collect::<Vec<_>>();
    directories.sort_by_key(|path| path.components().count());
    for path in directories {
        create_directory(&root, &path)?;
    }

    let mut files = pending
        .iter()
        .filter_map(|path| returned.get(path).map(|entry| (path.clone(), entry)))
        .filter(|(_, entry)| matches!(entry.fingerprint, Fingerprint::File { .. }))
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let write_options = WriteOptions {
        git_root: root_path,
        byte_limit,
    };
    for (path, entry) in files {
        let index_executable = index_modes.executable(&path)?;
        write_file(
            &write_options,
            &root,
            &path,
            entry,
            baseline.get(&path),
            index_executable,
            &mut hook,
        )?;
    }

    let mut deletions = pending
        .iter()
        .filter(|path| {
            !returned.contains_key(*path)
                && matches!(baseline.get(*path), Some(Fingerprint::File { .. }))
        })
        .cloned()
        .collect::<Vec<_>>();
    deletions.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for path in deletions {
        remove_entry(
            root_path,
            &root,
            &path,
            baseline.get(&path),
            byte_limit,
            index_modes.executable(&path)?,
            &mut hook,
        )?;
    }
    Ok(())
}

fn output_path_covers(
    allowed: &str,
    changed: &Path,
    baseline: &Manifest,
    returned: &BTreeMap<PathBuf, ReturnedEntry>,
) -> bool {
    if allowed == "." {
        return true;
    }
    let Some(allowed) = normalized_path(Path::new(allowed)) else {
        return false;
    };
    let Some(changed_key) = normalized_path(changed) else {
        return false;
    };
    path_is_same_or_child(&changed_key, &allowed)
        || (path_is_same_or_child(&allowed, &changed_key)
            && !baseline.contains_key(changed)
            && returned
                .get(changed)
                .is_some_and(|entry| entry.fingerprint == Fingerprint::Directory))
}

fn normalized_path(path: &Path) -> Option<String> {
    Some(
        path.to_str()?
            .split(['/', '\\'])
            // ASCII-only folding is conservative; differing Unicode case is
            // rejected instead of being granted output-path equivalence.
            .map(str::to_ascii_uppercase)
            .collect::<Vec<_>>()
            .join("\\"),
    )
}

fn path_is_same_or_child(candidate: &str, parent: &str) -> bool {
    candidate == parent
        || candidate
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('\\'))
}

fn changed_entries(
    baseline: &Manifest,
    returned: &BTreeMap<PathBuf, ReturnedEntry>,
) -> BTreeSet<PathBuf> {
    let mut all = baseline.keys().cloned().collect::<BTreeSet<_>>();
    all.extend(returned.keys().cloned());
    all.into_iter()
        .filter(|path| match (baseline.get(path), returned.get(path)) {
            (
                Some(Fingerprint::File {
                    sha256: old,
                    size: old_size,
                    mode: old_mode,
                }),
                Some(ReturnedEntry {
                    fingerprint:
                        Fingerprint::File {
                            sha256: new,
                            size: new_size,
                            mode: new_mode,
                        },
                    ..
                }),
            ) => old != new || old_size != new_size || old_mode != new_mode,
            (
                Some(Fingerprint::Directory),
                Some(ReturnedEntry {
                    fingerprint: Fingerprint::Directory,
                    ..
                }),
            ) => false,
            (None, None) => false,
            _ => true,
        })
        .collect()
}

fn matches_returned(current: Option<&Fingerprint>, returned: Option<&ReturnedEntry>) -> bool {
    match (current, returned.map(|entry| &entry.fingerprint)) {
        (Some(Fingerprint::Directory), Some(Fingerprint::Directory)) => true,
        (
            Some(Fingerprint::File {
                sha256: left_hash,
                size: left_size,
                mode: left_mode,
            }),
            Some(Fingerprint::File {
                sha256: right_hash,
                size: right_size,
                mode: right_mode,
            }),
        ) => left_hash == right_hash && left_size == right_size && left_mode == right_mode,
        (None, None) => true,
        _ => false,
    }
}

#[allow(dead_code)] // Checkpoints are consumed only by Windows-only race tests.
#[derive(Debug)]
enum Checkpoint {
    BeforeCreateInstall { target: PathBuf },
    AfterReplaceAside { target: PathBuf },
    AfterDeleteAside { target: PathBuf },
    AfterIndexModeUpdate { target: PathBuf },
}

#[cfg(test)]
#[path = "windows/tests.rs"]
mod tests;
