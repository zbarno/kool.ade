use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::artifacts::planning_store::{PlanningStore, StoreError};

mod location;
mod recovery;
mod revision;
#[cfg(test)]
mod tests;
use location::{acquire_lock, journal_path, remove_journal, store_identity, transaction_root};
pub(crate) use recovery::recover_store;
pub(crate) use revision::revision;

const JOURNAL: &str = "koolade-planning-store-transaction";
const LOCK: &str = "koolade-planning-store-lock";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    path: String,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Journal {
    root: String,
    entries: Vec<Entry>,
}

pub(crate) fn apply_store(
    store: &PlanningStore,
    changes: &[(String, Vec<u8>)],
    expected_revision: Option<&str>,
) -> Result<Vec<String>, StoreError> {
    apply_store_with_revision(store, changes, expected_revision).map(|(paths, _)| paths)
}

pub(crate) fn apply_store_with_revision(
    store: &PlanningStore,
    changes: &[(String, Vec<u8>)],
    expected_revision: Option<&str>,
) -> Result<(Vec<String>, String), StoreError> {
    apply_store_inner(store, changes, &[], expected_revision, None)
}

pub(crate) fn apply_store_with_removals(
    store: &PlanningStore,
    changes: &[(String, Vec<u8>)],
    removals: &[String],
    expected_revision: Option<&str>,
) -> Result<Vec<String>, StoreError> {
    apply_store_with_removals_and_revision(store, changes, removals, expected_revision)
        .map(|(paths, _)| paths)
}

pub(crate) fn apply_store_with_removals_and_revision(
    store: &PlanningStore,
    changes: &[(String, Vec<u8>)],
    removals: &[String],
    expected_revision: Option<&str>,
) -> Result<(Vec<String>, String), StoreError> {
    apply_store_inner(store, changes, removals, expected_revision, None)
}

#[cfg(test)]
fn apply_store_with_interruption(
    store: &PlanningStore,
    changes: &[(String, Vec<u8>)],
    expected_revision: Option<&str>,
    fail_after: Option<usize>,
) -> Result<(Vec<String>, String), StoreError> {
    apply_store_inner(store, changes, &[], expected_revision, fail_after)
}

fn apply_store_inner(
    store: &PlanningStore,
    changes: &[(String, Vec<u8>)],
    removals: &[String],
    expected_revision: Option<&str>,
    fail_after: Option<usize>,
) -> Result<(Vec<String>, String), StoreError> {
    let transaction_root = transaction_root(store)?;
    let root = store_identity(store);
    let journal_path = journal_path(&transaction_root, &root);
    let _lock = acquire_lock(&transaction_root, &root)?;

    let actual_revision = revision(store)?;
    if let Some(expected) = expected_revision
        && expected != actual_revision
    {
        return Err(StoreError::StaleRevision {
            expected: expected.to_owned(),
            actual: actual_revision,
        });
    }
    if journal_path.exists() {
        return Err(StoreError::UnavailableStore(
            "an unrecovered planning store transaction exists".into(),
        ));
    }

    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (relative, after) in changes {
        if !seen.insert(relative) {
            return Err(StoreError::MalformedState(format!(
                "duplicate planning path {relative}"
            )));
        }
        entries.push(entry_for(store, relative, Some(after.clone()))?);
    }
    for relative in removals {
        if !seen.insert(relative) {
            return Err(StoreError::MalformedState(format!(
                "duplicate planning path {relative}"
            )));
        }
        entries.push(entry_for(store, relative, None)?);
    }
    entries.retain(|entry| entry.before != entry.after);
    if entries.is_empty() {
        return Ok((Vec::new(), actual_revision));
    }

    let bytes = serde_json::to_vec(&Journal {
        root: root.clone(),
        entries: entries.clone(),
    })
    .map_err(|error| {
        StoreError::MalformedState(format!("cannot encode transaction journal: {error}"))
    })?;
    crate::artifacts::atomic_create_bytes(&journal_path, &bytes).map_err(|source| {
        StoreError::Io {
            path: journal_path.clone(),
            source,
        }
    })?;

    let result = (|| {
        for (index, entry) in entries.iter().enumerate() {
            if fail_after == Some(index) {
                return Err(StoreError::UnavailableStore(
                    "injected transaction interruption".into(),
                ));
            }
            apply_entry(store, entry)?;
        }
        Ok::<_, StoreError>(())
    })();
    if let Err(error) = result {
        if fail_after.is_some() {
            return Err(error);
        }
        restore(store, &entries)?;
        remove_journal(&journal_path)?;
        return Err(error);
    }

    remove_journal(&journal_path)?;
    let committed_revision = revision(store)?;
    Ok((
        entries.into_iter().map(|entry| entry.path).collect(),
        committed_revision,
    ))
}

fn entry_for(
    store: &PlanningStore,
    relative: &str,
    after: Option<Vec<u8>>,
) -> Result<Entry, StoreError> {
    let target = store.resolve(Path::new(relative))?;
    let before = match std::fs::read(&target) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(StoreError::Io {
                path: target,
                source,
            });
        }
    };
    Ok(Entry {
        path: relative.to_owned(),
        before,
        after,
    })
}

fn apply_entry(store: &PlanningStore, entry: &Entry) -> Result<(), StoreError> {
    let target = store.resolve(Path::new(&entry.path))?;
    match &entry.after {
        Some(bytes) => {
            crate::artifacts::atomic_write_bytes(&target, bytes).map_err(|source| StoreError::Io {
                path: target,
                source: std::io::Error::other(source),
            })
        }
        None => match std::fs::remove_file(&target) {
            Ok(()) => {
                crate::artifacts::sync_parent_directory(&target).map_err(|source| StoreError::Io {
                    path: target,
                    source,
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(StoreError::Io {
                path: target,
                source,
            }),
        },
    }
}

fn restore(store: &PlanningStore, entries: &[Entry]) -> Result<(), StoreError> {
    for entry in entries.iter().rev() {
        apply_entry(
            store,
            &Entry {
                path: entry.path.clone(),
                before: entry.after.clone(),
                after: entry.before.clone(),
            },
        )?;
    }
    Ok(())
}
