use super::*;
use crate::execution::session::AuthorizationGenerationVerifier;

#[derive(Debug)]
pub(super) struct ObservedAuthorization {
    pub(super) claim: Arc<ClaimLeaseHandle>,
    pub(super) statuses: Arc<Mutex<Vec<Option<AttemptStatus>>>>,
}

impl AuthorizationGenerationVerifier for ObservedAuthorization {
    fn generation(&self) -> String {
        self.claim.generation()
    }

    fn verify(&self, generation: &str) -> Result<(), crate::execution::errors::ExecutionError> {
        self.statuses.lock().unwrap().push(
            self.claim
                .execution_attempt_for_test()
                .map(|attempt| attempt.status),
        );
        <ClaimLeaseHandle as AuthorizationGenerationVerifier>::verify(&self.claim, generation)
    }

    fn begin_remote_attempt(
        &self,
        attempt_id: &str,
        worker_id: &str,
    ) -> Result<String, crate::execution::errors::ExecutionError> {
        self.claim.begin_remote_attempt(attempt_id, worker_id)
    }

    fn mark_remote_attempt_started(&self) -> Result<(), crate::execution::errors::ExecutionError> {
        self.claim.mark_remote_attempt_started()
    }

    fn finish_remote_attempt(&self) -> Result<(), crate::execution::errors::ExecutionError> {
        self.claim.finish_remote_attempt()
    }

    fn confirm_remote_attempt_lost(&self) -> Result<(), crate::execution::errors::ExecutionError> {
        self.claim.confirm_remote_attempt_lost()
    }
}
