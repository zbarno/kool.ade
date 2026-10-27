use crate::{
    app::bootstrap_remediation::AttemptContext,
    persistence::setup_tasks::{FailureCode, SetupTaskTransitionV1, apply_transition},
};

pub(super) fn fail_unapproved(
    attempt: &AttemptContext,
    failure: FailureCode,
) -> anyhow::Result<()> {
    apply_transition(SetupTaskTransitionV1::FailAttempt {
        task_id: attempt.task_id().to_owned(),
        operation_id: attempt.operation_id().to_owned(),
        failure,
        at_unix_ms: super::super::super::approval::now_unix_ms()?,
    })?;
    Ok(())
}
