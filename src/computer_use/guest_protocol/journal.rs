use crate::computer_use::ComputerActionV1;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use super::{GuestActionOutcomeV1, GuestProtocolErrorV1, types::canonical_uuid};

mod storage;
#[cfg(test)]
#[path = "journal/tests.rs"]
mod tests;
use storage::{action_digest, open_lock_file, read_state, write_state};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct JournalStateV1 {
    schema_version: u16,
    session_id: String,
    lease_generation: u64,
    last_action_sequence: u64,
    entries: BTreeMap<String, JournalEntryV1>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct JournalEntryV1 {
    request_id: String,
    action_id: String,
    action_sequence: u64,
    action_sha256: String,
    status: JournalEntryStatusV1,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "status",
    content = "outcome",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum JournalEntryStatusV1 {
    Pending,
    Completed(GuestActionOutcomeV1),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestJournalErrorV1 {
    InvalidIdentity,
    OwnershipMismatch,
    ReplayConflict,
    SequenceGap,
    JournalFull,
    OutcomeUnknown,
    Corrupt,
    Io,
}

impl std::fmt::Display for GuestJournalErrorV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "computer-use guest journal error: {self:?}")
    }
}

impl std::error::Error for GuestJournalErrorV1 {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuestReplayDecisionV1 {
    Execute,
    Replay(GuestActionOutcomeV1),
}

pub struct GuestActionJournalV1 {
    directory: PathBuf,
    session_id: String,
    lease_generation: u64,
    action_limit: u32,
    initial_action_sequence: u64,
}

impl GuestActionJournalV1 {
    /// `directory` must be the host-owned per-session runtime directory. The
    /// journal persists before side effects so uncertain requests never replay.
    pub fn open(
        directory: &Path,
        session_id: String,
        lease_generation: u64,
        initial_action_sequence: u64,
        action_limit: u32,
    ) -> Result<Self, GuestJournalErrorV1> {
        if !canonical_uuid(&session_id)
            || lease_generation == 0
            || action_limit == 0
            || initial_action_sequence > action_limit as u64
        {
            return Err(GuestJournalErrorV1::InvalidIdentity);
        }
        let metadata = fs::symlink_metadata(directory).map_err(|_| GuestJournalErrorV1::Io)?;
        if !metadata.file_type().is_dir() {
            return Err(GuestJournalErrorV1::OwnershipMismatch);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.uid() != unsafe { libc::geteuid() }
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(GuestJournalErrorV1::OwnershipMismatch);
            }
        }
        let journal = Self {
            directory: directory.to_path_buf(),
            session_id,
            lease_generation,
            action_limit,
            initial_action_sequence,
        };
        journal.with_state(|state| {
            if state.is_none() {
                *state = Some(JournalStateV1::new(
                    journal.session_id.clone(),
                    journal.lease_generation,
                    journal.initial_action_sequence,
                ));
            }
            Ok(())
        })?;
        Ok(journal)
    }

    pub fn begin(
        &self,
        request_id: &str,
        action: &ComputerActionV1,
    ) -> Result<GuestReplayDecisionV1, GuestJournalErrorV1> {
        if !canonical_uuid(request_id) || !canonical_uuid(&action.idempotency_key) {
            return Err(GuestJournalErrorV1::InvalidIdentity);
        }
        let action_sha256 = action_digest(action)?;
        let mut decision = GuestReplayDecisionV1::Execute;
        self.with_state(|state| {
            let state = state.as_mut().ok_or(GuestJournalErrorV1::Corrupt)?;
            if let Some(entry) = state.entries.get(request_id) {
                if entry.action_id != action.idempotency_key
                    || entry.action_sequence != action.action_sequence
                    || entry.action_sha256 != action_sha256
                {
                    return Err(GuestJournalErrorV1::ReplayConflict);
                }
                decision = match &entry.status {
                    JournalEntryStatusV1::Pending => {
                        return Err(GuestJournalErrorV1::OutcomeUnknown);
                    }
                    JournalEntryStatusV1::Completed(outcome) => {
                        GuestReplayDecisionV1::Replay(outcome.clone())
                    }
                };
                return Ok(());
            }
            if state.entries.len() >= self.action_limit as usize {
                return Err(GuestJournalErrorV1::JournalFull);
            }
            if state
                .entries
                .values()
                .any(|entry| entry.action_id == action.idempotency_key)
            {
                return Err(GuestJournalErrorV1::ReplayConflict);
            }
            if action.action_sequence != state.last_action_sequence.saturating_add(1) {
                return Err(GuestJournalErrorV1::SequenceGap);
            }
            state.last_action_sequence = action.action_sequence;
            state.entries.insert(
                request_id.to_owned(),
                JournalEntryV1 {
                    request_id: request_id.to_owned(),
                    action_id: action.idempotency_key.clone(),
                    action_sequence: action.action_sequence,
                    action_sha256,
                    status: JournalEntryStatusV1::Pending,
                },
            );
            Ok(())
        })?;
        Ok(decision)
    }

    pub fn complete(
        &self,
        request_id: &str,
        action: &ComputerActionV1,
        outcome: GuestActionOutcomeV1,
    ) -> Result<(), GuestJournalErrorV1> {
        let digest = action_digest(action)?;
        self.with_state(|state| {
            let entry = state
                .as_mut()
                .and_then(|state| state.entries.get_mut(request_id))
                .ok_or(GuestJournalErrorV1::OutcomeUnknown)?;
            if entry.action_id != action.idempotency_key
                || entry.action_sequence != action.action_sequence
                || entry.action_sha256 != digest
                || !matches!(entry.status, JournalEntryStatusV1::Pending)
            {
                return Err(GuestJournalErrorV1::ReplayConflict);
            }
            entry.status = JournalEntryStatusV1::Completed(outcome);
            Ok(())
        })
    }

    fn with_state<T>(
        &self,
        update: impl FnOnce(&mut Option<JournalStateV1>) -> Result<T, GuestJournalErrorV1>,
    ) -> Result<T, GuestJournalErrorV1> {
        let lock = open_lock_file(&self.lock_path())?;
        lock.lock_exclusive().map_err(|_| GuestJournalErrorV1::Io)?;
        let path = self.journal_path();
        let mut state = if path.exists() {
            Some(read_state(&path, &self.session_id, self.lease_generation)?)
        } else {
            None
        };
        let result = update(&mut state)?;
        if let Some(state) = state {
            write_state(&self.directory, &path, &state)?;
        }
        let _ = FileExt::unlock(&lock);
        Ok(result)
    }

    fn journal_path(&self) -> PathBuf {
        self.directory
            .join(format!("guest-actions-v1-{}.json", self.lease_generation))
    }

    fn lock_path(&self) -> PathBuf {
        self.directory
            .join(format!(".guest-actions-v1-{}.lock", self.lease_generation))
    }
}

impl JournalStateV1 {
    fn new(session_id: String, lease_generation: u64, initial_action_sequence: u64) -> Self {
        Self {
            schema_version: 1,
            session_id,
            lease_generation,
            last_action_sequence: initial_action_sequence,
            entries: BTreeMap::new(),
        }
    }
}

impl From<GuestJournalErrorV1> for GuestProtocolErrorV1 {
    fn from(error: GuestJournalErrorV1) -> Self {
        match error {
            GuestJournalErrorV1::OutcomeUnknown => Self::OutcomeUnknown,
            GuestJournalErrorV1::SequenceGap => Self::SequenceGap,
            GuestJournalErrorV1::ReplayConflict => Self::Replay,
            GuestJournalErrorV1::Io => Self::Io,
            GuestJournalErrorV1::InvalidIdentity
            | GuestJournalErrorV1::OwnershipMismatch
            | GuestJournalErrorV1::JournalFull
            | GuestJournalErrorV1::Corrupt => Self::Malformed,
        }
    }
}
