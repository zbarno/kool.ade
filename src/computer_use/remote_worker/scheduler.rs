use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

use super::{
    ComputerRemoteWorkerAdvertisementV1, ComputerRemoteWorkerEnrollmentV1,
    ComputerRemoteWorkerRequirementsV1, RemoteComputerWorkerEligibilityV1,
    assess_remote_computer_worker_v1,
};

const MAX_WORKER_ENTRIES: usize = 128;
const MAX_ACTIVE_SESSIONS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ResourceRequest {
    cpu_millis: u32,
    memory_bytes: u64,
    disk_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Reservation {
    worker_id: String,
    resources: ResourceRequest,
}

/// Controller-owned bounded capacity reservations. Scheduling always takes an
/// explicit approved worker ID and never falls back to local execution.
#[derive(Debug)]
pub struct ComputerRemoteWorkerSchedulerV1 {
    max_workers: usize,
    max_active_sessions: usize,
    reservations: Mutex<HashMap<String, Reservation>>,
}

#[derive(Debug)]
pub struct RemoteComputerWorkerPermitV1 {
    scheduler: Weak<ComputerRemoteWorkerSchedulerV1>,
    session_id: String,
    pub worker_id: String,
}

impl ComputerRemoteWorkerSchedulerV1 {
    pub fn new(max_workers: usize, max_active_sessions: usize) -> Option<Arc<Self>> {
        (max_workers > 0
            && max_workers <= MAX_WORKER_ENTRIES
            && max_active_sessions > 0
            && max_active_sessions <= MAX_ACTIVE_SESSIONS)
            .then(|| {
                Arc::new(Self {
                    max_workers,
                    max_active_sessions,
                    reservations: Mutex::new(HashMap::new()),
                })
            })
    }

    /// Production allocation remains locked until CU-16 has certified remote
    /// guest isolation, restart recovery, and two-host transport behavior.
    pub fn schedule_approved(
        self: &Arc<Self>,
        enrollment: &ComputerRemoteWorkerEnrollmentV1,
        advertisement: &ComputerRemoteWorkerAdvertisementV1,
        requirements: &ComputerRemoteWorkerRequirementsV1,
        approved_worker_id: &str,
        session_id: &str,
        now_unix_ms: u64,
    ) -> Result<RemoteComputerWorkerPermitV1, RemoteComputerWorkerEligibilityV1> {
        self.schedule_with_release_gate(
            enrollment,
            advertisement,
            requirements,
            approved_worker_id,
            session_id,
            now_unix_ms,
            crate::computer_use::remote_worker::remote_computer_use_release_gate_open_v1(),
        )
    }

    #[allow(clippy::too_many_arguments)] // Admission inputs and release certification remain independently testable.
    fn schedule_with_release_gate(
        self: &Arc<Self>,
        enrollment: &ComputerRemoteWorkerEnrollmentV1,
        advertisement: &ComputerRemoteWorkerAdvertisementV1,
        requirements: &ComputerRemoteWorkerRequirementsV1,
        approved_worker_id: &str,
        session_id: &str,
        now_unix_ms: u64,
        release_gate_open: bool,
    ) -> Result<RemoteComputerWorkerPermitV1, RemoteComputerWorkerEligibilityV1> {
        if !release_gate_open {
            return Err(RemoteComputerWorkerEligibilityV1::ReleaseGateClosed);
        }
        if approved_worker_id != advertisement.worker_id {
            return Err(RemoteComputerWorkerEligibilityV1::TargetApprovalMismatch);
        }
        if session_id.is_empty() || session_id.len() > 128 {
            return Err(RemoteComputerWorkerEligibilityV1::InvalidRequirements);
        }
        match assess_remote_computer_worker_v1(
            Some(enrollment),
            advertisement,
            requirements,
            now_unix_ms,
        ) {
            RemoteComputerWorkerEligibilityV1::Eligible => {}
            reason => return Err(reason),
        }

        let requested = ResourceRequest {
            cpu_millis: requirements.cpu_millis,
            memory_bytes: requirements.memory_bytes,
            disk_bytes: requirements.disk_bytes,
        };
        let mut reservations = self
            .reservations
            .lock()
            .map_err(|_| RemoteComputerWorkerEligibilityV1::SchedulerCapacityExceeded)?;
        if reservations.contains_key(session_id) {
            return Err(RemoteComputerWorkerEligibilityV1::SessionAlreadyScheduled);
        }
        if reservations.len() >= self.max_active_sessions {
            return Err(RemoteComputerWorkerEligibilityV1::SchedulerCapacityExceeded);
        }
        let worker_reservations = reservations
            .values()
            .filter(|reservation| reservation.worker_id == advertisement.worker_id)
            .collect::<Vec<_>>();
        let worker_limit = usize::min(
            advertisement.parallel_slots as usize,
            enrollment.max_parallel_sessions as usize,
        );
        let cpu_reserved = worker_reservations
            .iter()
            .map(|reservation| reservation.resources.cpu_millis)
            .sum::<u32>();
        let memory_reserved = worker_reservations
            .iter()
            .map(|reservation| reservation.resources.memory_bytes)
            .sum::<u64>();
        let disk_reserved = worker_reservations
            .iter()
            .map(|reservation| reservation.resources.disk_bytes)
            .sum::<u64>();
        if worker_reservations.len() >= worker_limit
            || cpu_reserved.saturating_add(requested.cpu_millis) > advertisement.cpu_millis
            || memory_reserved.saturating_add(requested.memory_bytes) > advertisement.memory_bytes
            || disk_reserved.saturating_add(requested.disk_bytes) > advertisement.disk_bytes
        {
            return Err(RemoteComputerWorkerEligibilityV1::CapacityUnavailable);
        }
        if !worker_reservations.is_empty() && reservations.len() >= self.max_workers {
            return Err(RemoteComputerWorkerEligibilityV1::SchedulerCapacityExceeded);
        }
        reservations.insert(
            session_id.to_owned(),
            Reservation {
                worker_id: advertisement.worker_id.clone(),
                resources: requested,
            },
        );
        Ok(RemoteComputerWorkerPermitV1 {
            scheduler: Arc::downgrade(self),
            session_id: session_id.to_owned(),
            worker_id: advertisement.worker_id.clone(),
        })
    }

    pub fn active_sessions(&self) -> usize {
        self.reservations
            .lock()
            .map(|reservations| reservations.len())
            .unwrap_or(usize::MAX)
    }
}

impl Drop for RemoteComputerWorkerPermitV1 {
    fn drop(&mut self) {
        if let Some(scheduler) = self.scheduler.upgrade()
            && let Ok(mut reservations) = scheduler.reservations.lock()
        {
            reservations.remove(&self.session_id);
        }
    }
}

#[cfg(test)]
#[path = "scheduler/tests.rs"]
mod tests;
