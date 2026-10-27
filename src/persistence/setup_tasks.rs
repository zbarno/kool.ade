//! Durable, machine-local bootstrap task state.
//!
//! Records are observations and audit metadata only. They never authorize a
//! host mutation or provide executable input to an assistant.

mod graph;
mod migration;
mod model;
mod storage;
mod transaction;

#[cfg(test)]
mod tests;

pub use graph::ordered_task_ids;
pub use model::{
    AttemptState, AuthorizationBindingV1, DependencyResolution, FailureCode,
    ImagePreparationReceiptV1, LocalTask, RemediationAttemptResultV1, RemediationProcessStatusV1,
    RemediationVerificationStatusV1, RemediationVerificationV1, SetupAttemptV1, SetupDependencyV1,
    SetupTaskEventV1, SetupTaskStateV1, SetupTaskStoreV1, SetupTaskTransitionV1, SetupTaskV1,
    State, Store, VerifiedResultV1, authorization_scope_digest,
};
pub use transaction::{apply_transition, begin_scan, load, reconcile_live, recover_after_restart};
pub(crate) use transaction::{
    begin_scan_at, reconcile_at_if, reconcile_live_if, recover_interrupted_at,
};

pub(crate) fn verify_private_acl(path: &std::path::Path) -> anyhow::Result<()> {
    storage::verify_private_acl(path)
}

pub(crate) fn machine_setup_state_dir() -> anyhow::Result<std::path::PathBuf> {
    #[cfg(test)]
    {
        Ok(crate::persistence::state_root().join("machine-setup"))
    }
    #[cfg(not(test))]
    {
        storage::state_dir()
    }
}

/// Persist a locked runtime profile selected for an actual workload. The next
/// machine setup scan binds the requirement to the currently verified engine
/// identity and architecture before publishing a preparation task.
pub fn require_image_profile(
    profile: crate::execution::docker::images::RuntimeProfile,
) -> anyhow::Result<SetupTaskStoreV1> {
    apply_transition(SetupTaskTransitionV1::RequireImageProfile { profile })
}

#[cfg(test)]
pub(crate) fn apply_transition_at_for_test(
    path: &std::path::Path,
    transition: SetupTaskTransitionV1,
) -> anyhow::Result<SetupTaskStoreV1> {
    transaction::apply_transition_at(path, transition)
}

#[cfg(test)]
pub(crate) fn begin_scan_at_for_test(path: &std::path::Path) -> anyhow::Result<u64> {
    transaction::begin_scan_at(path)
}

#[cfg(test)]
pub(crate) fn load_from_for_test(
    path: &std::path::Path,
) -> anyhow::Result<(SetupTaskStoreV1, bool)> {
    transaction::load_from(path)
}

#[cfg(test)]
pub(crate) fn save_to_for_test(
    path: &std::path::Path,
    store: &SetupTaskStoreV1,
) -> anyhow::Result<()> {
    transaction::save_to(path, store)
}

const MAX_STATE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TASKS: usize = 128;
const MAX_ATTEMPTS: usize = 24;
const MAX_EVENTS: usize = 24;
const TASK_STORE_LOCK: &str = "tasks.lock";

fn valid_id(id: &str) -> bool {
    id.len() <= 180
        && id.starts_with("setup:")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_'))
}
