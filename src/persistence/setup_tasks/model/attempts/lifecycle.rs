use super::super::*;

impl SetupTaskStoreV1 {
    pub(crate) fn await_verification(
        &mut self,
        task_id: &str,
        operation_id: &str,
        owner_instance_id: &str,
        at_unix_ms: u64,
    ) -> anyhow::Result<()> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| anyhow::anyhow!("setup task does not exist"))?;
        let attempt = active_attempt(
            task,
            operation_id,
            AttemptState::InProgress,
            owner_instance_id,
        )?;
        attempt.state = AttemptState::AwaitingVerification;
        attempt.updated_at_unix_ms = at_unix_ms;
        task.state = SetupTaskStateV1::AwaitingVerification;
        record(task, at_unix_ms, self.latest_scan_generation);
        Ok(())
    }

    pub(crate) fn fail_attempt(
        &mut self,
        task_id: &str,
        operation_id: &str,
        failure: FailureCode,
        owner_instance_id: &str,
        at_unix_ms: u64,
    ) -> anyhow::Result<()> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| anyhow::anyhow!("setup task does not exist"))?;
        let attempt = task
            .attempts
            .iter_mut()
            .find(|attempt| attempt.operation_id == operation_id)
            .ok_or_else(|| anyhow::anyhow!("setup operation does not exist"))?;
        anyhow::ensure!(
            matches!(
                attempt.state,
                AttemptState::NeedsApproval
                    | AttemptState::InProgress
                    | AttemptState::AwaitingVerification
            ),
            "setup operation cannot fail from its current state"
        );
        anyhow::ensure!(
            attempt.owner_instance_id.as_deref() == Some(owner_instance_id),
            "setup operation belongs to another app instance"
        );
        attempt.state = AttemptState::Failed;
        attempt.failure = Some(failure);
        attempt.authorization_binding = None;
        attempt.owner_instance_id = None;
        attempt.verified_result = None;
        attempt.updated_at_unix_ms = at_unix_ms;
        task.state = SetupTaskStateV1::Failed;
        task.verified_result = None;
        record(task, at_unix_ms, self.latest_scan_generation);
        Ok(())
    }

    pub(crate) fn cancel_attempt(
        &mut self,
        task_id: &str,
        operation_id: &str,
        owner_instance_id: &str,
        at_unix_ms: u64,
    ) -> anyhow::Result<()> {
        self.finish_attempt(
            task_id,
            operation_id,
            AttemptState::Cancelled,
            owner_instance_id,
            at_unix_ms,
        )
    }

    pub(crate) fn pause_attempt(
        &mut self,
        task_id: &str,
        operation_id: &str,
        owner_instance_id: &str,
        at_unix_ms: u64,
    ) -> anyhow::Result<()> {
        self.finish_attempt(
            task_id,
            operation_id,
            AttemptState::Paused,
            owner_instance_id,
            at_unix_ms,
        )
    }

    fn finish_attempt(
        &mut self,
        task_id: &str,
        operation_id: &str,
        next: AttemptState,
        owner_instance_id: &str,
        at_unix_ms: u64,
    ) -> anyhow::Result<()> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| anyhow::anyhow!("setup task does not exist"))?;
        let attempt = task
            .attempts
            .iter_mut()
            .find(|attempt| attempt.operation_id == operation_id)
            .ok_or_else(|| anyhow::anyhow!("setup operation does not exist"))?;
        anyhow::ensure!(
            matches!(
                attempt.state,
                AttemptState::NeedsApproval
                    | AttemptState::InProgress
                    | AttemptState::AwaitingVerification
            ),
            "setup operation cannot transition from its current state"
        );
        anyhow::ensure!(
            attempt.owner_instance_id.as_deref() == Some(owner_instance_id),
            "setup operation belongs to another app instance"
        );
        attempt.state = next;
        attempt.authorization_binding = None;
        attempt.owner_instance_id = None;
        attempt.updated_at_unix_ms = at_unix_ms;
        attempt.failure = (next == AttemptState::Cancelled).then_some(FailureCode::CancelledByUser);
        task.state = match next {
            AttemptState::Cancelled => SetupTaskStateV1::Cancelled,
            AttemptState::Paused => SetupTaskStateV1::Paused,
            _ => unreachable!("finish_attempt only accepts terminal pause/cancel states"),
        };
        task.verified_result = None;
        record(task, at_unix_ms, self.latest_scan_generation);
        Ok(())
    }

    pub(crate) fn recover_interrupted(
        &mut self,
        at_unix_ms: u64,
        current_owner: &str,
        mut owner_is_active: impl FnMut(&str) -> bool,
    ) -> bool {
        let mut changed = false;
        for task in self.tasks.values_mut() {
            let mut task_changed = false;
            for attempt in &mut task.attempts {
                if matches!(
                    attempt.state,
                    AttemptState::NeedsApproval
                        | AttemptState::InProgress
                        | AttemptState::AwaitingVerification
                ) {
                    let live_owner = attempt
                        .owner_instance_id
                        .as_deref()
                        .is_some_and(|owner| owner == current_owner || owner_is_active(owner));
                    if live_owner {
                        continue;
                    }
                    attempt.state = AttemptState::Interrupted;
                    attempt.failure = Some(FailureCode::InterruptedByRestart);
                    attempt.authorization_binding = None;
                    attempt.owner_instance_id = None;
                    attempt.verified_result = None;
                    attempt.updated_at_unix_ms = at_unix_ms;
                    task_changed = true;
                }
            }
            if task_changed {
                task.state = SetupTaskStateV1::Interrupted;
                task.verified_result = None;
                task.observed_at_unix_ms = at_unix_ms;
                task.scan_generation = self.latest_scan_generation.saturating_sub(1);
                record(task, at_unix_ms, self.latest_scan_generation);
                changed = true;
            }
        }
        changed
    }
}

fn active_attempt<'a>(
    task: &'a mut SetupTaskV1,
    operation_id: &str,
    expected: AttemptState,
    owner_instance_id: &str,
) -> anyhow::Result<&'a mut SetupAttemptV1> {
    let attempt = task
        .attempts
        .iter_mut()
        .find(|attempt| attempt.operation_id == operation_id)
        .ok_or_else(|| anyhow::anyhow!("setup operation does not exist"))?;
    anyhow::ensure!(
        attempt.state == expected,
        "setup operation is not in the expected state"
    );
    anyhow::ensure!(
        attempt.owner_instance_id.as_deref() == Some(owner_instance_id),
        "setup operation belongs to another app instance"
    );
    anyhow::ensure!(
        attempt.authorization_binding.is_some(),
        "active operation lacks approval metadata"
    );
    Ok(attempt)
}
