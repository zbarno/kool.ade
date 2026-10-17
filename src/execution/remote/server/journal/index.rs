use std::{collections::HashMap, fs::File, io};

use fs2::FileExt;

use super::{
    RequestJournal,
    record::{EntryState, JournalEntry, RequestIndexEntry, RequestIndexState},
    storage::{append_index, invalid_data, read_entries},
};

const MAX_LEGACY_EXECUTIONS_PER_RESERVE: usize = 1;

/// Keep only the newest transition for each request while rejecting an
/// identity change hidden among transitions.
pub(super) fn fold(entries: &[RequestIndexEntry]) -> io::Result<Vec<RequestIndexEntry>> {
    let mut latest = HashMap::<&str, &RequestIndexEntry>::new();
    for entry in entries {
        if let Some(previous) = latest.get(entry.request_id.as_str())
            && (previous.execution_id != entry.execution_id
                || previous.request_sha256 != entry.request_sha256
                || previous.created_unix_ms != entry.created_unix_ms
                || !valid_transition(&previous.state, &entry.state))
        {
            return Err(invalid_data(
                "remote request index has conflicting state transitions",
            ));
        }
        latest.insert(&entry.request_id, entry);
    }
    let mut folded = latest.into_values().cloned().collect::<Vec<_>>();
    folded.sort_by(|left, right| {
        left.created_unix_ms
            .cmp(&right.created_unix_ms)
            .then_with(|| left.request_id.cmp(&right.request_id))
    });
    Ok(folded)
}

fn valid_transition(previous: &RequestIndexState, next: &RequestIndexState) -> bool {
    if previous == next {
        return true;
    }
    matches!(
        (previous, next),
        (RequestIndexState::Unknown, RequestIndexState::Reserved)
            | (RequestIndexState::Unknown, RequestIndexState::Pending)
            | (
                RequestIndexState::Unknown,
                RequestIndexState::Completed { .. }
            )
            | (RequestIndexState::Unknown, RequestIndexState::Expired)
            | (RequestIndexState::Reserved, RequestIndexState::Pending)
            | (
                RequestIndexState::Reserved,
                RequestIndexState::Completed { .. }
            )
            | (RequestIndexState::Reserved, RequestIndexState::Expired)
            | (
                RequestIndexState::Pending,
                RequestIndexState::Completed { .. }
            )
    )
}

/// Classify a bounded batch of expired pre-state index rows. Unknown rows are
/// retained until classified, so incomplete migration can never free a fence.
pub(super) fn migrate_legacy(
    journal: &RequestJournal,
    index: &mut File,
    entries: &mut Vec<RequestIndexEntry>,
    now: u64,
    retention: u64,
) -> io::Result<()> {
    let folded = fold(entries)?;
    let Some(execution_id) = folded
        .iter()
        .filter(|entry| is_expired_legacy(entry, now, retention))
        .take(MAX_LEGACY_EXECUTIONS_PER_RESERVE)
        .next()
        .map(|entry| entry.execution_id.clone())
    else {
        return Ok(());
    };
    let candidates = folded
        .into_iter()
        .filter(|entry| {
            entry.execution_id == execution_id && is_expired_legacy(entry, now, retention)
        })
        .collect::<Vec<_>>();
    debug_assert!(!candidates.is_empty());

    let execution_path = journal.root.join(&execution_id);
    let directory_exists = match std::fs::symlink_metadata(&execution_path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => true,
        Ok(_) => return Err(invalid_data("remote execution journal directory is unsafe")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    let journal_exists = if directory_exists {
        match std::fs::symlink_metadata(execution_path.join("rpc.jsonl")) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => true,
            Ok(_) => {
                return Err(invalid_data(
                    "remote execution journal is not a regular file",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        }
    } else {
        false
    };
    let latest = if journal_exists {
        // Global index is already locked. Every path that takes both locks
        // acquires index first, then the execution lock.
        let (lock, mut log, _) = journal.open_locked(&execution_id)?;
        let log_entries = read_entries(&mut log)?;
        let wanted = candidates
            .iter()
            .map(|entry| entry.request_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let mut latest = HashMap::<&str, &JournalEntry>::new();
        for entry in &log_entries {
            if wanted.contains(entry.request_id.as_str()) {
                latest.insert(&entry.request_id, entry);
            }
        }
        FileExt::unlock(&lock)?;
        latest
            .into_iter()
            .map(|(request_id, entry)| (request_id.to_owned(), entry.clone()))
            .collect::<HashMap<_, _>>()
    } else {
        HashMap::new()
    };

    for mut entry in candidates {
        entry.state = match latest.get(&entry.request_id) {
            Some(journal_entry) if journal_entry.request_sha256 != entry.request_sha256 => {
                return Err(invalid_data(
                    "remote request index conflicts with its journal",
                ));
            }
            Some(JournalEntry {
                state: EntryState::Pending,
                ..
            }) => RequestIndexState::Pending,
            Some(JournalEntry {
                state: EntryState::Completed { .. },
                created_unix_ms,
                ..
            }) => {
                if now.saturating_sub(*created_unix_ms) <= retention {
                    RequestIndexState::Completed {
                        completed_unix_ms: *created_unix_ms,
                    }
                } else {
                    RequestIndexState::Expired
                }
            }
            None => RequestIndexState::Expired,
        };
        append_index(index, entry.clone())?;
        entries.push(entry);
    }
    Ok(())
}

fn is_expired_legacy(entry: &RequestIndexEntry, now: u64, retention: u64) -> bool {
    matches!(
        &entry.state,
        RequestIndexState::Unknown | RequestIndexState::Reserved
    ) && now.saturating_sub(entry.created_unix_ms) > retention
}

pub(super) fn retain(
    entry: &RequestIndexEntry,
    preserve_id: &str,
    now: u64,
    retention: u64,
) -> bool {
    if entry.request_id == preserve_id {
        return true;
    }
    match &entry.state {
        RequestIndexState::Unknown | RequestIndexState::Reserved | RequestIndexState::Pending => {
            true
        }
        RequestIndexState::Completed { completed_unix_ms } => {
            now.saturating_sub(*completed_unix_ms) <= retention
        }
        RequestIndexState::Expired => false,
    }
}
