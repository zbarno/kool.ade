use crate::persistence::setup_tasks::{SetupTaskStoreV1, SetupTaskTransitionV1, apply_transition};

pub(super) trait RemediationJournal {
    fn load(&mut self) -> anyhow::Result<SetupTaskStoreV1>;
    fn apply(&mut self, transition: SetupTaskTransitionV1) -> anyhow::Result<SetupTaskStoreV1>;
}

pub(super) struct DurableJournal;

impl RemediationJournal for DurableJournal {
    fn load(&mut self) -> anyhow::Result<SetupTaskStoreV1> {
        crate::persistence::setup_tasks::load()
    }

    fn apply(&mut self, transition: SetupTaskTransitionV1) -> anyhow::Result<SetupTaskStoreV1> {
        apply_transition(transition)
    }
}

#[cfg(test)]
pub(super) struct InMemoryJournal<'a> {
    store: &'a mut SetupTaskStoreV1,
    owner_instance_id: &'a str,
}

#[cfg(test)]
impl<'a> InMemoryJournal<'a> {
    pub(super) fn new(store: &'a mut SetupTaskStoreV1, owner_instance_id: &'a str) -> Self {
        Self {
            store,
            owner_instance_id,
        }
    }
}

#[cfg(test)]
impl RemediationJournal for InMemoryJournal<'_> {
    fn load(&mut self) -> anyhow::Result<SetupTaskStoreV1> {
        Ok(self.store.clone())
    }

    fn apply(&mut self, transition: SetupTaskTransitionV1) -> anyhow::Result<SetupTaskStoreV1> {
        use SetupTaskTransitionV1 as Transition;
        match transition {
            Transition::RecordRemediationResult {
                task_id,
                operation_id,
                result,
                at_unix_ms,
            } => self.store.record_remediation_result(
                &task_id,
                &operation_id,
                result,
                self.owner_instance_id,
                at_unix_ms,
            )?,
            Transition::AwaitVerification {
                task_id,
                operation_id,
                at_unix_ms,
            } => self.store.await_verification(
                &task_id,
                &operation_id,
                self.owner_instance_id,
                at_unix_ms,
            )?,
            Transition::FailAttempt {
                task_id,
                operation_id,
                failure,
                at_unix_ms,
            } => self.store.fail_attempt(
                &task_id,
                &operation_id,
                failure,
                self.owner_instance_id,
                at_unix_ms,
            )?,
            Transition::CancelAttempt {
                task_id,
                operation_id,
                at_unix_ms,
            } => self.store.cancel_attempt(
                &task_id,
                &operation_id,
                self.owner_instance_id,
                at_unix_ms,
            )?,
            Transition::PauseAttempt {
                task_id,
                operation_id,
                at_unix_ms,
            } => self.store.pause_attempt(
                &task_id,
                &operation_id,
                self.owner_instance_id,
                at_unix_ms,
            )?,
            Transition::RequireImageProfile { .. }
            | Transition::RequestAttempt { .. }
            | Transition::RecordImagePreparationReceipt { .. }
            | Transition::RecordAuthorization { .. } => {
                anyhow::bail!("test supervisor journal received an unexpected setup transition")
            }
        }
        Ok(self.store.clone())
    }
}
