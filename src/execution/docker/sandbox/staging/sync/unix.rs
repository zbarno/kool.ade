use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use super::super::archive::validate_relative;
use super::{Fingerprint, Manifest, ReturnedEntry};

mod fs;
mod write;
use fs::{create_directory, fingerprint_at, open_root_nofollow};
use write::{remove_entry, write_file};

pub(super) fn apply(
    root: &Path,
    baseline: &Manifest,
    returned: &BTreeMap<PathBuf, ReturnedEntry>,
    byte_limit: u64,
) -> Result<(), String> {
    let root = open_root_nofollow(root)?;
    let changed = changed_entries(baseline, returned);
    if changed.is_empty() {
        return Ok(());
    }

    // Preflight all paths through directory descriptors. Parent swaps and
    // symlink substitutions cannot redirect later operations outside root.
    // A competing cancellation/exec completion may already have applied this
    // exact container snapshot; treat that retry as idempotent while preserving
    // any host edit that differs from both the baseline and container result.
    let mut pending = BTreeSet::new();
    for path in &changed {
        let current = fingerprint_at(&root, path, byte_limit)?;
        if current.as_ref() == baseline.get(path) {
            pending.insert(path.clone());
        } else if current.as_ref() != returned.get(path).map(|entry| &entry.fingerprint) {
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
    for (path, entry) in files {
        write_file(&root, &path, entry, baseline.get(&path), byte_limit)?;
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
        remove_entry(&root, &path, baseline.get(&path), byte_limit)?;
    }
    Ok(())
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
                    ..
                }),
                Some(ReturnedEntry {
                    fingerprint:
                        Fingerprint::File {
                            sha256: new,
                            size: new_size,
                            ..
                        },
                    ..
                }),
            ) => old != new || old_size != new_size,
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
