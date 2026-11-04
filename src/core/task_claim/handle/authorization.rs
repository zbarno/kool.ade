use super::ClaimLeaseHandle;
use crate::{
    core::task_claim::{ClaimExecutionAttempt, ClaimExecutionAttemptStatus},
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
            return Err(policy_denied(
                "Execution authorization generation does not match the local task fence",
            ));
        }
        ClaimLeaseHandle::verify(self)
            .map_err(|error| policy_denied(&format!("The local task fence is invalid: {error}")))?;
        if let Some(attempt_id) = self
            .execution_attempt
            .lock()
            .map_err(|_| policy_denied("Local execution attempt fence is unavailable"))?
            .clone()
        {
            self.lease
                .lock()
                .map_err(|_| policy_denied("Local task fence lock is unavailable"))?
                .verify_execution_attempt(&attempt_id)
                .map_err(|error| policy_denied(&error.to_string()))?;
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
            status: ClaimExecutionAttemptStatus::Prepared,
        };
        self.lease
            .lock()
            .map_err(|_| policy_denied("Local task fence lock is unavailable"))?
            .begin_execution_attempt(attempt)
            .map_err(|error| policy_denied(&error.to_string()))?;
        *self
            .execution_attempt
            .lock()
            .map_err(|_| policy_denied("Local execution attempt fence is unavailable"))? =
            Some(attempt_id.to_owned());
        let generation = uuid::Uuid::new_v4().hyphenated().to_string();
        *self
            .authorization_generation
            .lock()
            .map_err(|_| policy_denied("Local authorization generation is unavailable"))? =
            generation.clone();
        Ok(generation)
    }

    fn mark_remote_attempt_started(&self) -> Result<(), ExecutionError> {
        self.set_attempt_status(ClaimExecutionAttemptStatus::Running)
    }

    fn finish_remote_attempt(&self) -> Result<(), ExecutionError> {
        self.set_attempt_status(ClaimExecutionAttemptStatus::Finished)
    }

    fn confirm_remote_attempt_lost(&self) -> Result<(), ExecutionError> {
        self.set_attempt_status(ClaimExecutionAttemptStatus::ConfirmedLost)?;
        *self
            .authorization_generation
            .lock()
            .map_err(|_| policy_denied("Local authorization generation is unavailable"))? =
            uuid::Uuid::new_v4().hyphenated().to_string();
        *self
            .execution_attempt
            .lock()
            .map_err(|_| policy_denied("Local execution attempt fence is unavailable"))? = None;
        Ok(())
    }
}

impl ClaimLeaseHandle {
    fn set_attempt_status(
        &self,
        status: ClaimExecutionAttemptStatus,
    ) -> Result<(), ExecutionError> {
        let Some(attempt_id) = self
            .execution_attempt
            .lock()
            .map_err(|_| policy_denied("Local execution attempt fence is unavailable"))?
            .clone()
        else {
            return Ok(());
        };
        self.lease
            .lock()
            .map_err(|_| policy_denied("Local task fence lock is unavailable"))?
            .set_execution_attempt_status(&attempt_id, status)
            .map_err(|error| policy_denied(&error.to_string()))
    }
}

fn policy_denied(message: &str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::PolicyDenied, message, false)
}
