use crate::execution::{errors::ExecutionError, session::AuthorizationGenerationVerifier};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

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

#[derive(Debug)]
pub(super) struct RevocableAuthorization {
    generation: String,
    active: Arc<AtomicBool>,
}

impl RevocableAuthorization {
    pub(super) fn new(generation: String) -> (Self, Arc<AtomicBool>) {
        let active = Arc::new(AtomicBool::new(true));
        (
            Self {
                generation,
                active: active.clone(),
            },
            active,
        )
    }
}

impl AuthorizationGenerationVerifier for RevocableAuthorization {
    fn generation(&self) -> String {
        self.generation.clone()
    }

    fn verify(&self, generation: &str) -> Result<(), ExecutionError> {
        if generation == self.generation && self.active.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::PolicyDenied,
                "test controller claim was replaced",
                false,
            ))
        }
    }
}
