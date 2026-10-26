use std::path::Path;

use crate::persistence::setup_tasks::{SetupTaskStoreV1, SetupTaskTransitionV1 as Transition};

use super::{owner, path, update_at};

/// Persist a typed lifecycle transition under the readiness store's file lock.
/// The caller never executes a host operation through this bookkeeping API.
pub fn apply_transition(transition: Transition) -> anyhow::Result<SetupTaskStoreV1> {
    apply_transition_at(&path()?, transition)
}

pub(crate) fn apply_transition_at(
    path: &Path,
    transition: Transition,
) -> anyhow::Result<SetupTaskStoreV1> {
    let owner_id = owner::ensure_current(path)?;
    update_at(path, |store| {
        apply_to_store(store, transition, &owner_id)?;
        Ok((store.clone(), true))
    })
}

fn apply_to_store(
    store: &mut SetupTaskStoreV1,
    transition: Transition,
    owner_id: &str,
) -> anyhow::Result<()> {
    match transition {
        Transition::RequireImageProfile { profile } => {
            store.require_image_profile(profile)?;
            Ok(())
        }
        Transition::RequestAttempt {
            task_id,
            operation_id,
            version,
            requested_scopes,
            at_unix_ms,
        } => store.request_attempt(
            &task_id,
            operation_id,
            version,
            requested_scopes,
            owner_id,
            at_unix_ms,
        ),
        Transition::RecordAuthorization {
            task_id,
            operation_id,
            binding,
            at_unix_ms,
        } => store.authorize_attempt(&task_id, &operation_id, binding, owner_id, at_unix_ms),
        Transition::RecordRemediationResult {
            task_id,
            operation_id,
            result,
            at_unix_ms,
        } => store.record_remediation_result(&task_id, &operation_id, result, owner_id, at_unix_ms),
        Transition::RecordImagePreparationReceipt {
            task_id,
            operation_id,
            receipt,
            at_unix_ms,
        } => store.record_image_preparation_receipt(
            &task_id,
            &operation_id,
            owner_id,
            receipt,
            at_unix_ms,
        ),
        Transition::AwaitVerification {
            task_id,
            operation_id,
            at_unix_ms,
        } => store.await_verification(&task_id, &operation_id, owner_id, at_unix_ms),
        Transition::FailAttempt {
            task_id,
            operation_id,
            failure,
            at_unix_ms,
        } => store.fail_attempt(&task_id, &operation_id, failure, owner_id, at_unix_ms),
        Transition::CancelAttempt {
            task_id,
            operation_id,
            at_unix_ms,
        } => store.cancel_attempt(&task_id, &operation_id, owner_id, at_unix_ms),
        Transition::PauseAttempt {
            task_id,
            operation_id,
            at_unix_ms,
        } => store.pause_attempt(&task_id, &operation_id, owner_id, at_unix_ms),
    }
}
