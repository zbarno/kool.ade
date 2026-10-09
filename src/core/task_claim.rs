//! Cross-clone claims stored as atomically-created refs on the repository remote.
mod git;
mod handle;
#[cfg(test)]
mod tests;
pub(crate) use handle::ClaimLeaseHandle;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CLAIM_PREFIX: &str = "refs/heads/koolade/claims/";
pub(crate) const HEARTBEAT_INTERVAL_SECONDS: u64 = 5 * 60;
const STALE_AFTER_SECONDS: i64 = 15 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimRecord {
    pub schema_version: u32,
    pub task_uid: String,
    pub owner: String,
    pub session_id: String,
    pub base_commit: String,
    pub claimed_at: i64,
    #[serde(default)]
    pub takeover_history: Box<Vec<ClaimHistoryEntry>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimHistoryEntry {
    pub owner: String,
    pub session_id: String,
    pub base_commit: String,
    pub claimed_at: i64,
    pub replaced_at: i64,
}

impl ClaimRecord {
    pub fn appears_stale(&self, now: i64) -> bool {
        now.saturating_sub(self.claimed_at) >= STALE_AFTER_SECONDS
    }
}

#[derive(Debug)]
pub struct ClaimLease {
    repo: PathBuf,
    remote: String,
    reference: String,
    object: String,
}

#[derive(Debug)]
pub struct ClaimAttempt {
    pub lease: Option<ClaimLease>,
    pub warning: Option<String>,
}

#[derive(Debug)]
pub struct ClaimRequest {
    repo: PathBuf,
    task_uid: String,
    base_commit: String,
    stale_session: Option<String>,
    allow_offline: bool,
}

#[derive(Debug)]
pub enum ClaimError {
    AlreadyClaimed { record: ClaimRecord, stale: bool },
    RemoteUnavailable(String),
    CoordinationRejected(String),
}

impl std::fmt::Display for ClaimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyClaimed { record, stale } => write!(
                f,
                "Task is claimed by {} (session {}) since {}; base {}; {}. Use the explicit stale-claim recovery action before taking it over.",
                record.owner,
                record.session_id,
                chrono::DateTime::from_timestamp(record.claimed_at, 0)
                    .map(|time| time.to_rfc3339())
                    .unwrap_or_else(|| record.claimed_at.to_string()),
                record.base_commit,
                if *stale {
                    "claim appears stale"
                } else {
                    "claim is active"
                }
            ),
            Self::RemoteUnavailable(detail) => {
                write!(f, "Cross-clone claim check failed: {detail}")
            }
            Self::CoordinationRejected(detail) => {
                write!(f, "Cross-clone claim was not acquired safely: {detail}")
            }
        }
    }
}

impl std::error::Error for ClaimError {}

impl ClaimLease {
    /// Atomically claim a stable task UID on `origin`. `Ok(None)` means there
    /// is no configured origin, so only the existing per-clone queue lock applies.
    pub fn acquire(
        repo: &std::path::Path,
        task_uid: &str,
        base_commit: &str,
    ) -> Result<Option<Self>, ClaimError> {
        git::acquire(repo, task_uid, base_commit)
    }

    /// Explicit compare-and-swap takeover. The caller must present the session
    /// currently shown to the operator; the remote claim must also be stale.
    pub fn take_over_stale(
        repo: &std::path::Path,
        task_uid: &str,
        expected_session_id: &str,
        base_commit: &str,
    ) -> Result<Self, ClaimError> {
        git::take_over_stale(repo, task_uid, expected_session_id, base_commit)
    }

    pub fn record(&self) -> Result<ClaimRecord, ClaimError> {
        git::read_claim(&self.repo, &self.remote, &self.reference, &self.object)
    }

    pub(crate) fn verify(&self) -> Result<(), ClaimError> {
        git::verify(&self.repo, &self.remote, &self.reference, &self.object)
    }

    pub(crate) fn refresh(&mut self) -> Result<(), ClaimError> {
        self.object = git::refresh(&self.repo, &self.remote, &self.reference, &self.object)?;
        Ok(())
    }

    pub(crate) fn fenced_push(
        &mut self,
        source: &std::path::Path,
        commit: &str,
        destination_ref: &str,
    ) -> Result<(), ClaimError> {
        self.object = git::fenced_push(
            &self.repo,
            &self.remote,
            &self.reference,
            &self.object,
            source,
            commit,
            destination_ref,
        )?;
        Ok(())
    }
}

impl ClaimRequest {
    pub(crate) fn new(
        repo: PathBuf,
        task_uid: String,
        base_commit: String,
        stale_session: Option<String>,
        allow_offline: bool,
    ) -> Self {
        Self {
            repo,
            task_uid,
            base_commit,
            stale_session,
            allow_offline,
        }
    }

    pub(crate) fn acquire(self) -> Result<ClaimAttempt, ClaimError> {
        let result = if let Some(session) = self.stale_session.as_deref() {
            ClaimLease::take_over_stale(&self.repo, &self.task_uid, session, &self.base_commit)
                .map(Some)
        } else {
            ClaimLease::acquire(&self.repo, &self.task_uid, &self.base_commit)
        };
        match result {
            Ok(lease) => {
                let warning = lease.is_none().then(|| "No origin remote is configured; this run is protected only by the local clone queue lock.".to_owned());
                Ok(ClaimAttempt { lease, warning })
            }
            Err(ClaimError::RemoteUnavailable(detail)) if self.allow_offline => Ok(ClaimAttempt {
                lease: None,
                warning: Some(format!(
                    "The shared remote claim could not be acquired; proceeding with the local clone lock only: {detail}"
                )),
            }),
            Err(error) => Err(error),
        }
    }
}

impl Drop for ClaimLease {
    fn drop(&mut self) {
        let _ = git::release(&self.repo, &self.remote, &self.reference, &self.object);
    }
}

pub(crate) fn reference(task_uid: &str) -> Result<String, ClaimError> {
    if task_uid.is_empty()
        || !task_uid
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(ClaimError::RemoteUnavailable(
            "Task UID is not safe for a Git ref".into(),
        ));
    }
    Ok(format!("{CLAIM_PREFIX}{task_uid}"))
}

#[cfg(test)]
fn stale_after_seconds() -> i64 {
    STALE_AFTER_SECONDS
}
