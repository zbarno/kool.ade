use crate::execution::{errors::ExecutionError, session::AuthorizationGenerationVerifier};

#[derive(Debug)]
pub(super) struct TestAuthorization(pub String);

impl AuthorizationGenerationVerifier for TestAuthorization {
    fn generation(&self) -> String {
        self.0.clone()
    }

    fn verify(&self, generation: &str) -> Result<(), ExecutionError> {
        if generation == self.0 {
            Ok(())
        } else {
            Err(ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::PolicyDenied,
                "test authorization generation changed",
                false,
            ))
        }
    }
}
