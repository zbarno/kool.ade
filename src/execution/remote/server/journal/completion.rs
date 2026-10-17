use std::io;

use fs2::FileExt;

use super::super::super::protocol::EnvelopeV1;
use super::{
    RequestJournal,
    record::{EntryState, JournalEntry, RequestIndexState},
    storage::{
        append_entry, append_index, fingerprint, invalid_data, now_unix_ms, read_entries,
        read_index,
    },
};

pub(super) fn complete(
    journal: &RequestJournal,
    request: &EnvelopeV1,
    response: &EnvelopeV1,
) -> io::Result<()> {
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

    // Combined operations consistently acquire index then execution locks.
    let (index_lock, mut index, _) = journal.open_index_locked()?;
    let folded = super::index::fold(&read_index(&mut index)?)?;
    let mut indexed = folded
        .into_iter()
        .find(|entry| entry.request_id == request.request_id)
        .ok_or_else(|| invalid_data("remote request journal state is inconsistent"))?;
    if indexed.execution_id != request.execution_id
        || indexed.request_sha256 != request_hash
        || matches!(&indexed.state, RequestIndexState::Expired)
    {
        return Err(invalid_data("remote request journal state is inconsistent"));
    }

    let (lock, mut log, _) = journal.open_locked(&request.execution_id)?;
    let entries = read_entries(&mut log)?;
    let latest = entries
        .iter()
        .rev()
        .find(|entry| entry.request_id == request.request_id);
    // A completed index paired with a pending journal cannot arise from the
    // write ordering below. Reject it before appending anything; the supported
    // recovery window is the inverse (durable journal completion, stale index).
    if matches!(&indexed.state, RequestIndexState::Completed { .. })
        && matches!(latest.map(|entry| &entry.state), Some(EntryState::Pending))
    {
        return Err(invalid_data("remote request journal state is inconsistent"));
    }
    let completed_at = match latest {
        Some(entry) if entry.request_sha256 != request_hash => {
            return Err(invalid_data("remote request journal state is inconsistent"));
        }
        Some(JournalEntry {
            state: EntryState::Pending,
            ..
        }) => {
            let timestamp = now_unix_ms();
            append_entry(
                &mut log,
                JournalEntry {
                    request_id: request.request_id.clone(),
                    request_sha256: request_hash,
                    created_unix_ms: timestamp,
                    state: EntryState::Completed {
                        response: response.clone(),
                    },
                },
            )?;
            timestamp
        }
        Some(JournalEntry {
            state: EntryState::Completed { response: existing },
            created_unix_ms,
            ..
        }) if existing == response => *created_unix_ms,
        _ => {
            return Err(invalid_data("remote request journal state is inconsistent"));
        }
    };
    if let RequestIndexState::Completed { completed_unix_ms } = &indexed.state {
        if *completed_unix_ms != completed_at {
            return Err(invalid_data("remote request journal state is inconsistent"));
        }
    } else {
        indexed.state = RequestIndexState::Completed {
            completed_unix_ms: completed_at,
        };
        append_index(&mut index, indexed)?;
    }
    FileExt::unlock(&lock)?;
    FileExt::unlock(&index_lock)?;
    Ok(())
}
