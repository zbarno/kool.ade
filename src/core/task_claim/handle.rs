use super::{ClaimError, ClaimLease};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
pub(crate) struct ClaimLeaseHandle {
    lease: Arc<Mutex<Option<ClaimLease>>>,
    lost: Arc<AtomicBool>,
}

impl ClaimLeaseHandle {
    pub(crate) fn new(lease: Option<ClaimLease>) -> Self {
        Self {
            lease: Arc::new(Mutex::new(lease)),
            lost: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn has_lease(&self) -> bool {
        self.lease.lock().is_ok_and(|lease| lease.is_some())
    }

    pub(crate) fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    pub(crate) fn verify(&self) -> Result<(), ClaimError> {
        if self.is_lost() {
            return Err(ClaimError::CoordinationRejected(
                "The remote task claim was already lost".into(),
            ));
        }
        let result = self
            .lease
            .lock()
            .map_err(|_| {
                ClaimError::CoordinationRejected("The remote task claim lock is unavailable".into())
            })?
            .as_ref()
            .ok_or_else(|| {
                ClaimError::CoordinationRejected(
                    "No remote task claim is available for publication".into(),
                )
            })?
            .verify();
        if result.is_err() {
            self.lost.store(true, Ordering::Release);
        }
        result
    }

    pub(crate) fn refresh(&self) -> Result<(), ClaimError> {
        let result = self
            .lease
            .lock()
            .map_err(|_| {
                ClaimError::CoordinationRejected("The remote task claim lock is unavailable".into())
            })?
            .as_mut()
            .map_or(Ok(()), ClaimLease::refresh);
        if result.is_err() {
            self.lost.store(true, Ordering::Release);
        }
        result
    }

    pub(crate) fn fenced_push(
        &self,
        source: &std::path::Path,
        commit: &str,
        destination_ref: &str,
    ) -> Result<(), ClaimError> {
        if self.is_lost() {
            return Err(ClaimError::CoordinationRejected(
                "The remote task claim was already lost".into(),
            ));
        }
        let result = self
            .lease
            .lock()
            .map_err(|_| {
                ClaimError::CoordinationRejected("The remote task claim lock is unavailable".into())
            })?
            .as_mut()
            .ok_or_else(|| {
                ClaimError::CoordinationRejected(
                    "No remote task claim is available for publication".into(),
                )
            })?
            .fenced_push(source, commit, destination_ref);
        if result.is_err() {
            self.lost.store(true, Ordering::Release);
        }
        result
    }
}
