use super::ClaimLeaseHandle;
use crate::{
    core::task_claim::ClaimExecutionAttempt,
    execution::{
        errors::{ExecutionError, ExecutionErrorKind},
        session::AuthorizationGenerationVerifier,
    },
};

impl AuthorizationGenerationVerifier for ClaimLeaseHandle {
    fn generation(&self) -> String {
        self.authorization_generation
            .lock()
            .map(|generation| generation.clone())
            .unwrap_or_default()
    }

    fn verify(&self, generation: &str) -> Result<(), ExecutionError> {
        if self.generation() != generation {
            return Err(ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "Execution authorization generation does not match the active task claim",
                false,
            ));
        }
        ClaimLeaseHandle::verify(self).map_err(|error| {
            ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                format!("The active task claim is no longer valid for execution: {error}"),
                false,
            )
        })?;
        if let Some(attempt_id) = self
            .execution_attempt
            .lock()
            .map_err(|_| fence_error("Remote execution attempt fence is unavailable"))?
            .clone()
        {
            self.lease
                .lock()
                .map_err(|_| fence_error("Remote task claim lock is unavailable"))?
                .as_ref()
                .ok_or_else(|| fence_error("No remote task claim is available for execution"))?
                .verify_execution_attempt(&attempt_id)
                .map_err(|error| {
                    ExecutionError::new(
                        ExecutionErrorKind::PolicyDenied,
                        format!("Remote execution attempt is no longer current: {error}"),
                        false,
                    )
                })?;
        }
        Ok(())
    }

    fn begin_remote_attempt(
        &self,
        attempt_id: &str,
        _worker_id: &str,
    ) -> Result<String, ExecutionError> {
        let attempt = ClaimExecutionAttempt {
            attempt_id: attempt_id.to_owned(),
            started_at_utc: chrono::Utc::now().timestamp(),
            status: crate::core::task_claim::ClaimExecutionAttemptStatus::Prepared,
        };
        let attempt_id = attempt.attempt_id.clone();
        self.lease
            .lock()
            .map_err(|_| fence_error("Remote task claim lock is unavailable"))?
            .as_mut()
            .ok_or_else(|| fence_error("No remote task claim is available for execution"))?
            .begin_execution_attempt(attempt)
            .map_err(|error| {
                ExecutionError::new(
                    ExecutionErrorKind::PolicyDenied,
                    format!("Could not fence the remote execution attempt: {error}"),
                    false,
                )
            })?;
        *self
            .execution_attempt
            .lock()
            .map_err(|_| fence_error("Remote execution attempt fence is unavailable"))? =
            Some(attempt_id);
        let generation = uuid::Uuid::new_v4().hyphenated().to_string();
        *self
            .authorization_generation
            .lock()
            .map_err(|_| fence_error("Remote authorization generation is unavailable"))? =
            generation.clone();
        Ok(generation)
    }

    fn mark_remote_attempt_started(&self) -> Result<(), ExecutionError> {
        self.set_attempt_status(crate::core::task_claim::ClaimExecutionAttemptStatus::Running)
    }

    fn finish_remote_attempt(&self) -> Result<(), ExecutionError> {
        self.set_attempt_status(crate::core::task_claim::ClaimExecutionAttemptStatus::Finished)
    }

    fn confirm_remote_attempt_lost(&self) -> Result<(), ExecutionError> {
        self.set_attempt_status(crate::core::task_claim::ClaimExecutionAttemptStatus::ConfirmedLost)
    }
}

impl ClaimLeaseHandle {
    fn set_attempt_status(
        &self,
        status: crate::core::task_claim::ClaimExecutionAttemptStatus,
    ) -> Result<(), ExecutionError> {
        let Some(attempt_id) = self
            .execution_attempt
            .lock()
            .map_err(|_| fence_error("Remote execution attempt fence is unavailable"))?
            .clone()
        else {
            return Ok(());
        };
        self.lease
            .lock()
            .map_err(|_| fence_error("Remote task claim lock is unavailable"))?
            .as_mut()
            .ok_or_else(|| fence_error("No remote task claim is available for execution"))?
            .set_execution_attempt_status(&attempt_id, status)
            .map_err(|error| {
                ExecutionError::new(
                    ExecutionErrorKind::PolicyDenied,
                    format!("Could not update the remote execution attempt fence: {error}"),
                    false,
                )
            })
    }
}

fn fence_error(message: &str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::PolicyDenied, message, false)
}
