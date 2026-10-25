//! Durable, machine-local bootstrap task state.
//!
//! Records are observations and audit metadata only. They never authorize a
//! host mutation or provide executable input to an assistant.

mod graph;
mod migration;
mod model;
mod transaction;

#[cfg(test)]
mod tests;

pub use graph::ordered_task_ids;
pub use model::{
    AttemptState, AuthorizationBindingV1, DependencyResolution, FailureCode, LocalTask,
    SetupAttemptV1, SetupDependencyV1, SetupTaskEventV1, SetupTaskStateV1, SetupTaskStoreV1,
    SetupTaskTransitionV1, SetupTaskV1, State, Store, VerifiedResultV1, authorization_scope_digest,
};
pub use transaction::{apply_transition, begin_scan, load, reconcile_live, recover_after_restart};

const MAX_STATE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TASKS: usize = 128;
const MAX_ATTEMPTS: usize = 24;
const MAX_EVENTS: usize = 24;

fn valid_id(id: &str) -> bool {
    id.len() <= 180
        && id.starts_with("setup:")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_'))
}
