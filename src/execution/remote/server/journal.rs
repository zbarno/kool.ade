//! Durable idempotency journal for authenticated worker RPC requests.

use std::{fs::File, io, path::PathBuf};

use super::super::protocol::EnvelopeV1;
use fs2::FileExt;

mod record;
mod storage;

#[cfg(test)]
#[path = "journal/tests.rs"]
mod tests;

use record::{EntryState, JournalEntry, RequestIndexEntry};
use storage::{
    append_entry, append_index, create_private_dir, fingerprint, invalid_data, now_unix_ms,
    open_private, read_entries, read_index,
};

const MAX_JOURNAL_ENTRIES: usize = 16_384;
const REPLAY_RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1000;

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
        let request_hash = fingerprint(request)?;
        let (index_lock, mut index) = self.open_index_locked()?;
        let index_entries = read_index(&mut index)?;
        let mut indexed = false;
        for entry in &index_entries {
            if entry.request_id == request.request_id {
                if entry.execution_id != request.execution_id
                    || entry.request_sha256 != request_hash
                {
                    return Err(invalid_data(
                        "remote request ID conflicts with a prior request",
                    ));
                }
                indexed = true;
            }
        }
        if !indexed {
            if index_entries.len() >= MAX_JOURNAL_ENTRIES {
                return Err(invalid_data("remote request index is full"));
            }
            append_index(
                &mut index,
                RequestIndexEntry {
                    request_id: request.request_id.clone(),
                    execution_id: request.execution_id.clone(),
                    request_sha256: request_hash.clone(),
                    created_unix_ms: now_unix_ms(),
                },
            )?;
        }

        let (lock, mut log) = self.open_locked(&request.execution_id)?;
        let entries = read_entries(&mut log)?;
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
            if now_unix_ms().saturating_sub(entry.created_unix_ms) > REPLAY_RETENTION_MS {
                return Ok(Lookup::Expired);
            }
            return match entry.state {
                EntryState::Pending => Ok(Lookup::Pending),
                EntryState::Completed { response } => Ok(Lookup::Replay(response)),
            };
        }
        if entries.len() >= MAX_JOURNAL_ENTRIES {
            return Err(invalid_data("remote execution request journal is full"));
        }
        append_entry(
            &mut log,
            JournalEntry {
                request_id: request.request_id.clone(),
                request_sha256: request_hash,
                created_unix_ms: now_unix_ms(),
                state: EntryState::Pending,
            },
        )?;
        FileExt::unlock(&lock)?;
        FileExt::unlock(&index_lock)?;
        Ok(Lookup::New)
    }

    /// Append the response only after dispatch has produced it. A crash while
    /// pending is deliberately not retried because execution may have begun.
    pub fn complete(&self, request: &EnvelopeV1, response: &EnvelopeV1) -> io::Result<()> {
        let (lock, mut log) = self.open_locked(&request.execution_id)?;
        let entries = read_entries(&mut log)?;
        let request_hash = fingerprint(request)?;
        if response.request_id != request.request_id
            || response.execution_id != request.execution_id
            || response.sequence != request.sequence
            || response.version != request.version
        {
            return Err(invalid_data(
                "remote request journal response identity is invalid",
            ));
        }
        let pending = entries.iter().any(|entry| {
            entry.request_id == request.request_id
                && entry.request_sha256 == request_hash
                && matches!(entry.state, EntryState::Pending)
        });
        if !pending {
            return Err(invalid_data("remote request journal state is inconsistent"));
        }
        append_entry(
            &mut log,
            JournalEntry {
                request_id: request.request_id.clone(),
                request_sha256: request_hash,
                created_unix_ms: now_unix_ms(),
                state: EntryState::Completed {
                    response: response.clone(),
                },
            },
        )?;
        FileExt::unlock(&lock)?;
        Ok(())
    }

    fn open_locked(&self, execution_id: &str) -> io::Result<(File, File)> {
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
        let log = match open_private(&directory.join("rpc.jsonl"), true) {
            Ok(log) => log,
            Err(error) => {
                let _ = FileExt::unlock(&lock);
                return Err(error);
            }
        };
        Ok((lock, log))
    }

    fn open_index_locked(&self) -> io::Result<(File, File)> {
        create_private_dir(&self.root)?;
        let lock = open_private(&self.root.join("request-index.lock"), true)?;
        FileExt::lock_exclusive(&lock)?;
        let index = match open_private(&self.root.join("request-index.jsonl"), true) {
            Ok(index) => index,
            Err(error) => {
                let _ = FileExt::unlock(&lock);
                return Err(error);
            }
        };
        Ok((lock, index))
    }
}
