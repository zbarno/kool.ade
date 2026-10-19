//! Durable idempotency journal for authenticated worker RPC requests.

use std::{fs::File, io, path::PathBuf};

use super::super::protocol::EnvelopeV1;
use fs2::FileExt;

mod completion;
mod index;
mod record;
mod storage;

#[cfg(test)]
#[path = "journal/tests.rs"]
mod tests;

use record::{EntryState, JournalEntry, RequestIndexEntry, RequestIndexState};
use storage::{
    append_entry, append_index, compact_entries, compact_index, create_private_dir, fingerprint,
    invalid_data, now_unix_ms, open_private, read_entries, read_index,
};

const MAX_JOURNAL_ENTRIES: usize = 16_384;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Lookup {
    New,
    Pending,
    Expired,
    Replay(EnvelopeV1),
}

#[derive(Clone, Debug)]
pub(super) struct RequestJournal {
    root: PathBuf,
}

impl RequestJournal {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Reserve the globally unique request ID and its per-execution record
    /// before dispatch. Exact completed retries replay the saved response.
    pub fn reserve(&self, request: &EnvelopeV1) -> io::Result<Lookup> {
        if request.kind == super::super::protocol::EnvelopeKindV1::PrivateFileChunk {
            return Err(invalid_data(
                "secret-bearing private file chunks cannot enter the worker journal",
            ));
        }
        let request_hash = fingerprint(request)?;
        let (index_lock, index, index_path) = self.open_index_locked()?;
        let mut index = index;
        let now = now_unix_ms();
        let mut existing_index = read_index(&mut index)?;
        index::migrate_legacy(
            self,
            &mut index,
            &mut existing_index,
            now,
            crate::persistence::remote_requests::REPLAY_RETENTION_MS,
        )?;
        let (mut index, mut index_entries) = compact_index(
            index,
            &index_path,
            &existing_index,
            &request.request_id,
            now,
            crate::persistence::remote_requests::REPLAY_RETENTION_MS,
        )?;
        let prior_index = index_entries
            .iter()
            .find(|entry| entry.request_id == request.request_id)
            .cloned();
        if let Some(entry) = &prior_index {
            if entry.execution_id != request.execution_id || entry.request_sha256 != request_hash {
                return Err(invalid_data(
                    "remote request ID conflicts with a prior request",
                ));
            }
            if matches!(&entry.state, RequestIndexState::Expired) {
                return Ok(Lookup::Expired);
            }
            if matches!(&entry.state, RequestIndexState::Completed { completed_unix_ms }
                if now.saturating_sub(*completed_unix_ms) > crate::persistence::remote_requests::REPLAY_RETENTION_MS)
            {
                return Ok(Lookup::Expired);
            }
        } else {
            if index_entries.len() >= MAX_JOURNAL_ENTRIES {
                return Err(invalid_data("remote request index is full"));
            }
            let reservation = RequestIndexEntry {
                request_id: request.request_id.clone(),
                execution_id: request.execution_id.clone(),
                request_sha256: request_hash.clone(),
                created_unix_ms: now,
                state: RequestIndexState::Reserved,
            };
            append_index(&mut index, reservation.clone())?;
            index_entries.push(reservation);
        }

        let (lock, log, log_path) = self.open_locked(&request.execution_id)?;
        let mut log = log;
        let existing_entries = read_entries(&mut log)?;
        let (mut log, entries) = compact_entries(
            log,
            &log_path,
            &existing_entries,
            &request.request_id,
            now,
            crate::persistence::remote_requests::REPLAY_RETENTION_MS,
        )?;
        let mut prior_entry = None;
        for entry in &entries {
            if entry.request_id == request.request_id {
                if entry.request_sha256 != request_hash {
                    return Err(invalid_data(
                        "remote request ID conflicts with a prior request",
                    ));
                }
                prior_entry = Some(entry.clone());
            }
        }
        if let Some(entry) = prior_entry {
            if now.saturating_sub(entry.created_unix_ms)
                > crate::persistence::remote_requests::REPLAY_RETENTION_MS
            {
                return Ok(Lookup::Expired);
            }
            let target_state = match &entry.state {
                EntryState::Pending => RequestIndexState::Pending,
                EntryState::Completed { .. } => RequestIndexState::Completed {
                    completed_unix_ms: entry.created_unix_ms,
                },
            };
            let mut indexed = index_entries
                .iter()
                .find(|row| row.request_id == request.request_id)
                .cloned()
                .ok_or_else(|| invalid_data("remote request index is inconsistent"))?;
            match &indexed.state {
                RequestIndexState::Completed { completed_unix_ms }
                    if target_state == RequestIndexState::Pending
                        || *completed_unix_ms != entry.created_unix_ms =>
                {
                    return Err(invalid_data("remote request journal state is inconsistent"));
                }
                _ => {}
            }
            if indexed.state != target_state {
                indexed.state = target_state;
                append_index(&mut index, indexed)?;
            }
            FileExt::unlock(&lock)?;
            FileExt::unlock(&index_lock)?;
            return match entry.state {
                EntryState::Pending => Ok(Lookup::Pending),
                EntryState::Completed { response } => Ok(Lookup::Replay(response)),
            };
        }
        if index_entries.iter().any(|row| {
            row.request_id == request.request_id
                && matches!(
                    &row.state,
                    RequestIndexState::Pending | RequestIndexState::Completed { .. }
                )
        }) {
            return Err(invalid_data("remote request journal state is inconsistent"));
        }
        if entries.len() >= MAX_JOURNAL_ENTRIES {
            return Err(invalid_data("remote execution request journal is full"));
        }
        append_entry(
            &mut log,
            JournalEntry {
                request_id: request.request_id.clone(),
                request_sha256: request_hash,
                created_unix_ms: now,
                state: EntryState::Pending,
            },
        )?;
        let mut indexed = index_entries
            .iter()
            .find(|row| row.request_id == request.request_id)
            .cloned()
            .ok_or_else(|| invalid_data("remote request index is inconsistent"))?;
        indexed.state = RequestIndexState::Pending;
        append_index(&mut index, indexed)?;
        FileExt::unlock(&lock)?;
        FileExt::unlock(&index_lock)?;
        Ok(Lookup::New)
    }

    /// Append the response only after dispatch has produced it. A crash while
    /// pending is deliberately not retried because execution may have begun.
    pub fn complete(&self, request: &EnvelopeV1, response: &EnvelopeV1) -> io::Result<()> {
        if request.kind == super::super::protocol::EnvelopeKindV1::PrivateFileChunk
            || response.kind == super::super::protocol::EnvelopeKindV1::PrivateFileChunk
        {
            return Err(invalid_data(
                "secret-bearing private file chunks cannot enter the worker journal",
            ));
        }
        completion::complete(self, request, response)
    }

    fn open_locked(&self, execution_id: &str) -> io::Result<(File, File, PathBuf)> {
        if uuid::Uuid::parse_str(execution_id)
            .map_or(true, |id| id.hyphenated().to_string() != execution_id)
        {
            return Err(invalid_data("remote execution journal identity is invalid"));
        }
        create_private_dir(&self.root)?;
        let directory = self.root.join(execution_id);
        create_private_dir(&directory)?;
        let lock = open_private(&directory.join("rpc.lock"), true)?;
        FileExt::lock_exclusive(&lock)?;
        let path = directory.join("rpc.jsonl");
        let log = match open_private(&path, true) {
            Ok(log) => log,
            Err(error) => {
                let _ = FileExt::unlock(&lock);
                return Err(error);
            }
        };
        Ok((lock, log, path))
    }

    fn open_index_locked(&self) -> io::Result<(File, File, PathBuf)> {
        create_private_dir(&self.root)?;
        let lock = open_private(&self.root.join("request-index.lock"), true)?;
        FileExt::lock_exclusive(&lock)?;
        let path = self.root.join("request-index.jsonl");
        let index = match open_private(&path, true) {
            Ok(index) => index,
            Err(error) => {
                let _ = FileExt::unlock(&lock);
                return Err(error);
            }
        };
        Ok((lock, index, path))
    }
}
