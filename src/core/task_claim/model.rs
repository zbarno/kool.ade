use std::{fs::File, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimExecutionAttemptStatus {
    Prepared,
    Running,
    Finished,
    ConfirmedLost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimExecutionAttempt {
    pub attempt_id: String,
    pub started_at_utc: i64,
    pub status: ClaimExecutionAttemptStatus,
}

#[derive(Debug)]
pub struct ClaimLease {
    pub(super) lock: File,
    pub(super) lock_path: PathBuf,
    pub(super) token: String,
    pub(super) execution_attempt: Option<ClaimExecutionAttempt>,
}

#[derive(Debug)]
pub struct ClaimRequest {
    pub(super) state_root: PathBuf,
    pub(super) project_uid: uuid::Uuid,
    pub(super) task_uid: String,
}

#[derive(Debug, Clone)]
pub enum ClaimError {
    AlreadyClaimed,
    CoordinationRejected(String),
    LocalStorage(String),
}

impl std::fmt::Display for ClaimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyClaimed => write!(
                f,
                "This task is already running in another Kool.ad/e process on this installation"
            ),
            Self::CoordinationRejected(detail) => write!(
                f,
                "Local task fencing rejected the operation: {}",
                crate::error::redact_secrets(detail)
            ),
            Self::LocalStorage(detail) => write!(
                f,
                "Private local task fencing is unavailable: {}",
                crate::error::redact_secrets(detail)
            ),
        }
    }
}

impl std::error::Error for ClaimError {}
