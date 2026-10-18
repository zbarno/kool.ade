//! Durable, operator-local lifecycle records for container executions.
//!
//! Records live outside project clones and shared planning repositories so a
//! restart can reconcile an execution without publishing runtime metadata.

mod journal;
mod owner;
mod record;
mod retention;
mod store;

#[cfg(test)]
#[path = "execution_records/tests.rs"]
mod tests;

pub use journal::{append_event, journal_cursor, load_events};
pub use owner::{owner_instance_id, try_task_lock, unfinished_for_owner};
pub use record::{
    DockerResourceUsageV1, ExecutionArtifactV1, ExecutionCleanupStateV1, ExecutionPhaseTimingsV1,
    ExecutionRecordStateV1, ExecutionRecordV1, ExecutionWorkspaceEntryKindV1,
    ExecutionWorkspaceEntryV1,
};
pub use store::{latest_for_task, load, save};
