use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::computer_use::{
    ComputerActionActorV1, ComputerActionV1, ComputerIdentityV1, ComputerNetworkProfileV1,
    ComputerOperationV1, ComputerResourceBudgetV1, ComputerSessionClassV1, ComputerSessionV1,
};

mod storage;

fn is_terminal(kind: SessionEventKindV1) -> bool {
    matches!(
        kind,
        SessionEventKindV1::SessionReleased
            | SessionEventKindV1::SessionStopped
            | SessionEventKindV1::SessionDismissed
            | SessionEventKindV1::StartupFailed
    )
}

const JOURNAL_FILE: &str = "session-events-v1.jsonl";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum SessionEventKindV1 {
    SessionRequested,
    OverlayPrepared,
    BackendPrepareStarted,
    BackendPrepared,
    BootStarted,
    GuestReady,
    ActionIntent,
    ActionApplied,
    ActionFailed,
    OutcomeUnknown,
    AgentPaused,
    HumanTakeover,
    AgentResumed,
    SessionSuspended,
    SessionResumed,
    SessionInterrupted,
    SessionStopped,
    SessionReleased,
    SessionDismissed,
    StartupFailed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct SessionJournalEventV1 {
    pub schema_version: u16,
    pub sequence: u64,
    pub event_id: String,
    pub kind: SessionEventKindV1,
    pub reconciled_from: Option<SessionEventKindV1>,
    pub boot_fingerprint: Option<String>,
    pub identity: ComputerIdentityV1,
    pub session_class: ComputerSessionClassV1,
    pub agent_role: String,
    pub lease_id: String,
    pub lease_generation: u64,
    pub approval_generation: u64,
    pub cancellation_generation: u64,
    pub image_digest: String,
    pub policy_sha256: String,
    pub network_profile: ComputerNetworkProfileV1,
    pub allowed_apps: Vec<String>,
    pub resource_budget: ComputerResourceBudgetV1,
    pub expires_at_unix_ms: u64,
    pub action_sequence: Option<u64>,
    pub action_id: Option<String>,
    pub action_actor: Option<ComputerActionActorV1>,
    pub action_operation: Option<ComputerOperationV1>,
    pub occurred_at_unix_ms: u64,
}

pub(super) struct ComputerUseSessionJournalV1 {
    root: PathBuf,
}

impl ComputerUseSessionJournalV1 {
    pub fn open(root: &Path) -> Result<Self, &'static str> {
        std::fs::create_dir_all(root).map_err(|_| "session_journal_unavailable")?;
        storage::verify_private_dir(root)?;
        let journal = Self {
            root: root.to_path_buf(),
        };
        journal.with_locked_file(|file| {
            storage::read_events(file)?;
            Ok(())
        })?;
        Ok(journal)
    }

    pub fn append_session(
        &self,
        session: &ComputerSessionV1,
        agent_role: &str,
        kind: SessionEventKindV1,
        action: Option<&ComputerActionV1>,
        occurred_at_unix_ms: u64,
    ) -> Result<u64, &'static str> {
        self.append_session_with_boot_fingerprint(
            session,
            agent_role,
            kind,
            action,
            occurred_at_unix_ms,
            None,
        )
    }

    pub fn append_session_with_boot_fingerprint(
        &self,
        session: &ComputerSessionV1,
        agent_role: &str,
        kind: SessionEventKindV1,
        action: Option<&ComputerActionV1>,
        occurred_at_unix_ms: u64,
        boot_fingerprint: Option<String>,
    ) -> Result<u64, &'static str> {
        let policy_bytes =
            serde_json::to_vec(&session.policy).map_err(|_| "session_journal_invalid_event")?;
        let policy_sha256 = format!("sha256:{:x}", sha2::Sha256::digest(policy_bytes));
        self.append(SessionJournalEventV1 {
            schema_version: 1,
            sequence: 0,
            event_id: uuid::Uuid::new_v4().to_string(),
            kind,
            reconciled_from: None,
            boot_fingerprint,
            identity: session.identity.clone(),
            session_class: session.session_class,
            agent_role: agent_role.to_owned(),
            lease_id: session.lease.lease_id.clone(),
            lease_generation: session.lease.generation,
            approval_generation: session.approval_generation,
            cancellation_generation: session.cancellation_generation,
            image_digest: session.image_digest.clone(),
            policy_sha256,
            network_profile: session.network_profile,
            allowed_apps: session.allowed_apps.iter().cloned().collect(),
            resource_budget: session.policy.resource_budget.clone(),
            expires_at_unix_ms: session.lease.expires_at_unix_ms,
            action_sequence: action.map(|action| action.action_sequence),
            action_id: action.map(|action| action.idempotency_key.clone()),
            action_actor: action.map(|action| action.actor),
            action_operation: action.map(|action| action.operation),
            occurred_at_unix_ms,
        })
    }

    pub fn append(&self, mut event: SessionJournalEventV1) -> Result<u64, &'static str> {
        storage::validate_event(&event)?;
        event.sequence = 0;
        self.with_locked_file(|file| {
            let mut events = storage::read_events(file)?;
            if events.len() >= storage::MAX_JOURNAL_EVENTS {
                return Err("session_journal_capacity_exceeded");
            }
            let next_sequence = events
                .last()
                .map_or(Some(1), |last| last.sequence.checked_add(1))
                .ok_or("session_journal_sequence_exhausted")?;
            event.sequence = next_sequence;
            storage::validate_event(&event)?;
            let mut bytes =
                serde_json::to_vec(&event).map_err(|_| "session_journal_invalid_event")?;
            bytes.push(b'\n');
            if file
                .metadata()
                .map_err(|_| "session_journal_unavailable")?
                .len()
                .saturating_add(bytes.len() as u64)
                > storage::MAX_JOURNAL_BYTES as u64
            {
                return Err("session_journal_capacity_exceeded");
            }
            file.write_all(&bytes)
                .map_err(|_| "session_journal_unavailable")?;
            file.sync_data()
                .map_err(|_| "session_journal_unavailable")?;
            events.clear();
            Ok(next_sequence)
        })
    }

    pub fn latest_events(&self) -> Result<Vec<SessionJournalEventV1>, &'static str> {
        self.with_locked_file(|file| {
            let events = storage::read_events(file)?;
            let mut latest = std::collections::BTreeMap::new();
            for event in events {
                latest.insert(event.identity.session_id.clone(), event);
            }
            Ok(latest.into_values().collect())
        })
    }

    pub fn reconcile_after_restart(
        &self,
        now_unix_ms: u64,
    ) -> Result<Vec<SessionJournalEventV1>, &'static str> {
        let latest = self.latest_events()?;
        let mut interrupted = Vec::new();
        for mut event in latest {
            if is_terminal(event.kind) || event.kind == SessionEventKindV1::SessionInterrupted {
                continue;
            }
            let previous_kind = event.kind;
            event.kind = SessionEventKindV1::SessionInterrupted;
            event.reconciled_from = Some(previous_kind);
            event.event_id = uuid::Uuid::new_v4().to_string();
            event.occurred_at_unix_ms = now_unix_ms;
            self.append(event.clone())?;
            interrupted.push(event);
        }
        Ok(interrupted)
    }

    pub fn dismiss(&self, session_id: &str, now_unix_ms: u64) -> Result<(), &'static str> {
        let event = self
            .latest_events()?
            .into_iter()
            .find(|event| event.identity.session_id == session_id)
            .ok_or("session_not_found")?;
        let mut dismissed = event;
        dismissed.kind = SessionEventKindV1::SessionDismissed;
        dismissed.reconciled_from = None;
        dismissed.event_id = uuid::Uuid::new_v4().to_string();
        dismissed.occurred_at_unix_ms = now_unix_ms;
        dismissed.action_sequence = None;
        dismissed.action_id = None;
        dismissed.action_actor = None;
        dismissed.action_operation = None;
        self.append(dismissed)?;
        Ok(())
    }

    fn with_locked_file<T>(
        &self,
        run: impl FnOnce(&mut File) -> Result<T, &'static str>,
    ) -> Result<T, &'static str> {
        let path = self.root.join(JOURNAL_FILE);
        let mut options = OpenOptions::new();
        options.create(true).append(true).read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let mut file = options
            .open(path)
            .map_err(|_| "session_journal_unavailable")?;
        storage::verify_private_file(&file)?;
        file.lock_exclusive()
            .map_err(|_| "session_journal_unavailable")?;
        let result = run(&mut file);
        let _ = FileExt::unlock(&file);
        result
    }
}

#[cfg(test)]
#[path = "session_journal/tests.rs"]
mod tests;
