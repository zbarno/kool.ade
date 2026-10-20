use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimRecord {
    pub schema_version: u32,
    #[serde(default)]
    pub project_uid: String,
    pub task_uid: String,
    pub owner: String,
    #[serde(default)]
    pub operator_id: String,
    pub session_id: String,
    #[serde(default)]
    pub code_repository_id: String,
    pub base_commit: String,
    #[serde(default, alias = "claimedAt")]
    pub issued_at_utc: i64,
    #[serde(default)]
    pub expires_at_utc: i64,
    #[serde(default)]
    pub takeover_history: Box<Vec<ClaimHistoryEntry>>,
    /// Current remote worker attempt. Replacing it fences sessions from the prior attempt.
    #[serde(default)]
    pub execution_attempt: Option<ClaimExecutionAttempt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimExecutionAttempt {
    pub attempt_id: String,
    pub started_at_utc: i64,
    #[serde(default)]
    pub status: ClaimExecutionAttemptStatus,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimExecutionAttemptStatus {
    #[default]
    Prepared,
    Running,
    Finished,
    ConfirmedLost,
}

impl ClaimExecutionAttempt {
    pub(crate) fn validate(&self) -> Result<(), ClaimError> {
        if !uuid::Uuid::parse_str(&self.attempt_id)
            .is_ok_and(|id| id.hyphenated().to_string() == self.attempt_id)
            || self.started_at_utc <= 0
        {
            return Err(ClaimError::CoordinationRejected(
                "Remote execution attempt metadata is invalid".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimHistoryEntry {
    pub owner: String,
    pub session_id: String,
    #[serde(default)]
    pub code_repository_id: String,
    pub base_commit: String,
    pub claimed_at: i64,
    pub replaced_at: i64,
}

impl ClaimRecord {
    pub fn appears_stale(&self, now: i64) -> bool {
        let expires_at = if self.expires_at_utc == 0 {
            // Claims written by the prior schema expire on the same lease
            // interval during cutover.
            self.issued_at_utc
                .saturating_add(super::STALE_AFTER_SECONDS)
        } else {
            self.expires_at_utc
        };
        now >= self.issued_at_utc && expires_at >= self.issued_at_utc && now >= expires_at
    }

    pub fn expires_at(&self) -> i64 {
        if self.expires_at_utc == 0 {
            self.issued_at_utc
                .saturating_add(super::STALE_AFTER_SECONDS)
        } else {
            self.expires_at_utc
        }
    }
}

#[derive(Debug)]
pub struct ClaimLease {
    pub(super) repo: PathBuf,
    pub(super) remote: String,
    pub(super) reference: String,
    pub(super) object: String,
    pub(super) project_uid: String,
    pub(super) task_uid: String,
    pub(super) code_repository_id: String,
}

#[derive(Debug)]
pub struct ClaimAttempt {
    pub lease: Option<ClaimLease>,
    pub warning: Option<String>,
    pub(crate) mode: ClaimAttemptMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaimAttemptMode {
    Shared,
    LocalOnly,
    UnsafeOffline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaimScope {
    LocalOnly,
    SharedOptional,
    SharedRequired,
}

#[derive(Debug)]
pub struct ClaimRequest {
    pub(super) repo: PathBuf,
    pub(super) code_repo: PathBuf,
    pub(super) project_uid: uuid::Uuid,
    pub(super) task_uid: String,
    pub(super) code_repository_id: String,
    pub(super) base_commit: String,
    pub(super) stale_session: Option<String>,
    pub(super) scope: ClaimScope,
    pub(super) allow_offline: bool,
}

#[derive(Debug, Clone)]
pub enum ClaimError {
    AlreadyClaimed {
        record: Box<ClaimRecord>,
        stale: bool,
    },
    RemoteUnavailable(String),
    CoordinationRejected(String),
}

impl std::fmt::Display for ClaimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyClaimed { record, stale } => {
                let operator = if record.operator_id.is_empty() {
                    "unknown"
                } else {
                    &record.operator_id
                };
                write!(
                    f,
                    "Task is claimed by {} (operator {}, session {}) until {}; base {}; {}. Use the explicit stale-claim recovery action before taking it over.",
                    record.owner,
                    operator,
                    record.session_id,
                    chrono::DateTime::from_timestamp(record.expires_at(), 0)
                        .map(|time| time.to_rfc3339())
                        .unwrap_or_else(|| record.expires_at().to_string()),
                    record.base_commit,
                    if *stale {
                        "claim appears stale"
                    } else {
                        "claim is active"
                    }
                )
            }
            Self::RemoteUnavailable(detail) => {
                write!(
                    f,
                    "Cross-clone claim check failed: {}",
                    crate::error::redact_secrets(detail)
                )
            }
            Self::CoordinationRejected(detail) => {
                write!(
                    f,
                    "Cross-clone claim was not acquired safely: {}",
                    crate::error::redact_secrets(detail)
                )
            }
        }
    }
}

impl std::error::Error for ClaimError {}
