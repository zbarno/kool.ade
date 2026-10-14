use super::{ClaimError, ClaimLease};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
pub(crate) struct ClaimLeaseHandle {
    lease: Arc<Mutex<Option<ClaimLease>>>,
    lost: Arc<AtomicBool>,
    lost_reason: Arc<Mutex<Option<ClaimError>>>,
    publication_blocked: bool,
    authorization_generation: String,
}

impl std::fmt::Debug for ClaimLeaseHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClaimLeaseHandle")
            .field("has_lease", &self.has_lease())
            .field("authorization_generation", &self.authorization_generation)
            .finish()
    }
}

impl ClaimLeaseHandle {
    pub(crate) fn new(lease: ClaimLease) -> Self {
        Self {
            lease: Arc::new(Mutex::new(Some(lease))),
            lost: Arc::new(AtomicBool::new(false)),
            lost_reason: Arc::new(Mutex::new(None)),
            publication_blocked: false,
            authorization_generation: uuid::Uuid::new_v4().hyphenated().to_string(),
        }
    }

    pub(crate) fn block_publication() -> Self {
        Self {
            lease: Arc::new(Mutex::new(None)),
            lost: Arc::new(AtomicBool::new(false)),
            lost_reason: Arc::new(Mutex::new(None)),
            publication_blocked: true,
            authorization_generation: uuid::Uuid::new_v4().hyphenated().to_string(),
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
            return Err(self
                .lost_reason
                .lock()
                .ok()
                .and_then(|reason| reason.clone())
                .unwrap_or_else(|| {
                    ClaimError::CoordinationRejected(
                        "The remote task claim was already lost".into(),
                    )
                }));
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
        if let Err(error) = &result {
            if let Ok(mut reason) = self.lost_reason.lock() {
                reason.get_or_insert_with(|| error.clone());
            }
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
        if let Err(error) = &result {
            if let Ok(mut reason) = self.lost_reason.lock() {
                reason.get_or_insert_with(|| error.clone());
            }
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
            return Err(self
                .lost_reason
                .lock()
                .ok()
                .and_then(|reason| reason.clone())
                .unwrap_or_else(|| {
                    ClaimError::CoordinationRejected(
                        "The remote task claim was already lost".into(),
                    )
                }));
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
        if let Err(error) = &result {
            if let Ok(mut reason) = self.lost_reason.lock() {
                reason.get_or_insert_with(|| error.clone());
            }
            self.lost.store(true, Ordering::Release);
        }
        result
    }
}

impl crate::execution::session::AuthorizationGenerationVerifier for ClaimLeaseHandle {
    fn generation(&self) -> String {
        self.authorization_generation.clone()
    }

    fn verify(&self, generation: &str) -> Result<(), crate::execution::errors::ExecutionError> {
        if generation != self.authorization_generation {
            return Err(crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::PolicyDenied,
                "Execution authorization generation does not match the active task claim",
                false,
            ));
        }
        ClaimLeaseHandle::verify(self).map_err(|error| {
            crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::PolicyDenied,
                format!("The active task claim is no longer valid for execution: {error}"),
                false,
            )
        })
    }
}
