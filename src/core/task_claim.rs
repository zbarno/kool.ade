//! Process-safe task execution fences stored under private local application state.
mod claim_lease;
mod handle;
mod model;
mod request;
#[cfg(test)]
mod tests;

pub(crate) use handle::ClaimLeaseHandle;
pub use model::{
    ClaimError, ClaimExecutionAttempt, ClaimExecutionAttemptStatus, ClaimLease, ClaimRequest,
};

/// Probe the real advisory task lock without creating a lock file or changing
/// its token. File presence alone is not evidence of a live owner.
pub fn is_claimed_by_another_process(
    state_root: &std::path::Path,
    project_uid: uuid::Uuid,
    task_uid: &str,
) -> Result<bool, ClaimError> {
    ClaimLease::is_held_by_another_process(state_root, project_uid, task_uid)
}
