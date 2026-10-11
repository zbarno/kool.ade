use super::{ClaimError, ClaimLease};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
pub(crate) struct ClaimLeaseHandle {
    lease: Arc<Mutex<Option<ClaimLease>>>,
    lost: Arc<AtomicBool>,
    publication_blocked: bool,
}

impl ClaimLeaseHandle {
    pub(crate) fn new(lease: ClaimLease) -> Self {
        Self {
            lease: Arc::new(Mutex::new(Some(lease))),
            lost: Arc::new(AtomicBool::new(false)),
            publication_blocked: false,
        }
    }

    pub(crate) fn block_publication() -> Self {
        Self {
            lease: Arc::new(Mutex::new(None)),
            lost: Arc::new(AtomicBool::new(false)),
            publication_blocked: true,
        }
    }

    pub(crate) fn has_lease(&self) -> bool {
        self.lease.lock().is_ok_and(|lease| lease.is_some())
    }

    pub(crate) fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    pub(crate) fn verify(&self) -> Result<(), ClaimError> {
        if self.publication_blocked {
            self.lost.store(true, Ordering::Release);
            return Err(ClaimError::CoordinationRejected(
                "Publication is disabled for an explicitly approved offline run".into(),
            ));
        }
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
        if self.publication_blocked {
            return Err(ClaimError::CoordinationRejected(
                "A shared claim cannot be renewed for an offline run".into(),
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
                    "No remote task claim is available for renewal".into(),
                )
            })?
            .refresh();
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
        if self.publication_blocked {
            self.lost.store(true, Ordering::Release);
            return Err(ClaimError::CoordinationRejected(
                "Publication is disabled for an explicitly approved offline run".into(),
            ));
        }
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
