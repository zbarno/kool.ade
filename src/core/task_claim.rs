//! Cross-clone claims stored as atomically-created refs on the planning remote.
mod claim_lease;
mod git;
mod handle;
mod model;
mod request;
#[cfg(test)]
mod tests;

pub(crate) use handle::ClaimLeaseHandle;
pub use model::{
    ClaimAttempt, ClaimError, ClaimExecutionAttempt, ClaimExecutionAttemptStatus,
    ClaimHistoryEntry, ClaimLease, ClaimRecord, ClaimRequest,
};
pub(crate) use model::{ClaimAttemptMode, ClaimScope};

const CLAIM_PREFIX: &str = "refs/heads/koolade/claims/";
const STALE_AFTER_SECONDS: i64 = 15 * 60;
pub(crate) const HEARTBEAT_INTERVAL_SECONDS: u64 = 5 * 60;

pub(crate) fn reference(project_uid: uuid::Uuid, task_uid: &str) -> Result<String, ClaimError> {
    validate_task_uid(task_uid)?;
    Ok(format!(
        "{CLAIM_PREFIX}{}/{task_uid}",
        project_uid.hyphenated()
    ))
}

pub(crate) fn legacy_reference(task_uid: &str) -> Result<String, ClaimError> {
    validate_task_uid(task_uid)?;
    Ok(format!("{CLAIM_PREFIX}{task_uid}"))
}

fn validate_task_uid(task_uid: &str) -> Result<(), ClaimError> {
    if task_uid.is_empty()
        || !task_uid
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(ClaimError::CoordinationRejected(
            "Task UID is not safe for a Git ref".into(),
        ));
    }
    Ok(())
}
