use crate::execution::errors::ExecutionError;

/// App-owned claim/capability fence captured by an execution session and
/// rechecked before execution and before results can be accepted.
pub trait AuthorizationGenerationVerifier: Send + Sync + std::fmt::Debug {
    fn generation(&self) -> String;
    fn verify(&self, generation: &str) -> Result<(), ExecutionError>;

    /// Persist and activate a new remote worker attempt before opening its session.
    fn begin_remote_attempt(
        &self,
        _attempt_id: &str,
        _worker_id: &str,
    ) -> Result<String, ExecutionError> {
        Ok(self.generation())
    }

    fn mark_remote_attempt_started(&self) -> Result<(), ExecutionError> {
        Ok(())
    }

    fn finish_remote_attempt(&self) -> Result<(), ExecutionError> {
        Ok(())
    }

    /// Fence a worker attempt after the authenticated worker reports its
    /// durable execution record as interrupted.
    fn confirm_remote_attempt_lost(&self) -> Result<(), ExecutionError> {
        Ok(())
    }
}
