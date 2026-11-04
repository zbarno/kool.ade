#[cfg(test)]
use super::ClaimExecutionAttempt;
use super::{ClaimError, ClaimLease};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

mod authorization;

#[derive(Clone)]
pub(crate) struct ClaimLeaseHandle {
    lease: Arc<Mutex<ClaimLease>>,
    lost: Arc<AtomicBool>,
    lost_reason: Arc<Mutex<Option<ClaimError>>>,
    authorization_generation: Arc<Mutex<String>>,
    execution_attempt: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for ClaimLeaseHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClaimLeaseHandle")
            .field("has_lease", &true)
            .field(
                "authorization_generation",
                &self.authorization_generation.lock().ok().as_deref(),
            )
            .finish()
    }
}

impl ClaimLeaseHandle {
    pub(crate) fn new(lease: ClaimLease) -> Self {
        Self {
            lease: Arc::new(Mutex::new(lease)),
            lost: Arc::new(AtomicBool::new(false)),
            lost_reason: Arc::new(Mutex::new(None)),
            authorization_generation: Arc::new(Mutex::new(
                uuid::Uuid::new_v4().hyphenated().to_string(),
            )),
            execution_attempt: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn execution_attempt_for_test(&self) -> Option<ClaimExecutionAttempt> {
        self.lease.lock().ok()?.execution_attempt.clone()
    }

    pub(crate) fn verify(&self) -> Result<(), ClaimError> {
        if self.is_lost() {
            return Err(self
                .lost_reason
                .lock()
                .ok()
                .and_then(|reason| reason.clone())
                .unwrap_or_else(|| {
                    ClaimError::CoordinationRejected("The local task fence was already lost".into())
                }));
        }
        let result = self
            .lease
            .lock()
            .map_err(|_| ClaimError::LocalStorage("Local task fence lock is unavailable".into()))?
            .verify();
        if let Err(error) = &result {
            if let Ok(mut reason) = self.lost_reason.lock() {
                reason.get_or_insert_with(|| error.clone());
            }
            self.lost.store(true, Ordering::Release);
        }
        result
    }
}
