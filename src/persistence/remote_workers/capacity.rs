//! Cross-process reservations for enrolled remote worker capacity.

use std::{path::PathBuf, time::Duration};

const LEASE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

mod reserve;
mod scheduler_cursor;
mod store;
#[cfg(test)]
pub(super) use store::STORE;
use store::{Reservation, Store, with_store};

pub use reserve::reserve_capacity;
#[cfg(test)]
use reserve::reserve_capacity_at;
pub use scheduler_cursor::{
    order_auto_workers, record_auto_worker_selection, reserve_auto_capacity,
};

/// A durable worker slot reservation. Dropping releases it; a crashed owner
/// is recovered after the bounded lease expiry.
pub struct RemoteWorkerCapacityLease {
    root: PathBuf,
    reservation: Reservation,
}

/// Fresh load and enrolled identity used to reserve an Auto route atomically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoWorkerCapacityCandidate {
    pub worker_id: String,
    pub host_key_fingerprint: String,
    pub max_jobs: u32,
    pub observed_running_jobs: u32,
}

impl std::fmt::Debug for RemoteWorkerCapacityLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteWorkerCapacityLease")
            .field("worker_id", &self.reservation.worker_id)
            .field("id", &self.reservation.id)
            .finish()
    }
}

impl RemoteWorkerCapacityLease {
    pub fn worker_id(&self) -> &str {
        &self.reservation.worker_id
    }

    pub fn matches_worker(&self, worker_id: &str, host_key_fingerprint: &str) -> bool {
        self.reservation.worker_id == worker_id
            && self.reservation.host_key_fingerprint == host_key_fingerprint
    }

    /// Recheck admission against a fresh worker load and current limit while
    /// holding the cross-process reservation lock.
    pub fn revalidate(&self, max_jobs: u32, observed_running_jobs: u32) -> anyhow::Result<()> {
        anyhow::ensure!((1..=256).contains(&max_jobs), "worker job limit is invalid");
        with_store(&self.root, |store| {
            let now = now_unix_ms();
            prune(store, now);
            anyhow::ensure!(
                store.reservations.iter().any(|item| {
                    item.id == self.reservation.id
                        && item.worker_id == self.reservation.worker_id
                        && item.host_key_fingerprint == self.reservation.host_key_fingerprint
                }),
                "remote worker capacity reservation expired or changed"
            );
            let active = store
                .reservations
                .iter()
                .filter(|item| {
                    item.worker_id == self.reservation.worker_id
                        && item.host_key_fingerprint == self.reservation.host_key_fingerprint
                        && item.pending_start
                })
                .count();
            anyhow::ensure!(
                observed_running_jobs.saturating_add(active as u32) <= max_jobs,
                "remote worker capacity changed before the session was established"
            );
            Ok(())
        })
    }

    /// Extends the crash-recovery window while this controller still owns the
    /// live execution session.
    pub fn renew(&mut self) -> anyhow::Result<()> {
        let now = now_unix_ms();
        with_store(&self.root, |store| {
            prune(store, now);
            let index = store
                .reservations
                .iter()
                .position(|item| item.id == self.reservation.id)
                .ok_or_else(|| anyhow::anyhow!("remote worker capacity lease expired"))?;
            let item = &store.reservations[index];
            anyhow::ensure!(
                item.worker_id == self.reservation.worker_id
                    && item.host_key_fingerprint == self.reservation.host_key_fingerprint,
                "remote worker capacity lease identity changed"
            );
            let expires_at_unix_ms = now.saturating_add(ttl_ms());
            store.reservations[index].expires_at_unix_ms = expires_at_unix_ms;
            self.reservation.expires_at_unix_ms = expires_at_unix_ms;
            Ok(())
        })
    }

    /// Atomically reacquire a pending start slot against fresh worker load.
    pub fn prepare_start(
        &mut self,
        max_jobs: u32,
        observed_running_jobs: u32,
    ) -> anyhow::Result<()> {
        anyhow::ensure!((1..=256).contains(&max_jobs), "worker job limit is invalid");
        let now = now_unix_ms();
        with_store(&self.root, |store| {
            prune(store, now);
            let index = store
                .reservations
                .iter()
                .position(|item| item.id == self.reservation.id)
                .ok_or_else(|| anyhow::anyhow!("remote worker capacity lease expired"))?;
            let item = &store.reservations[index];
            anyhow::ensure!(
                item.worker_id == self.reservation.worker_id
                    && item.host_key_fingerprint == self.reservation.host_key_fingerprint,
                "remote worker capacity lease identity changed"
            );
            let was_pending = item.pending_start;
            let pending = store
                .reservations
                .iter()
                .filter(|item| {
                    item.worker_id == self.reservation.worker_id
                        && item.host_key_fingerprint == self.reservation.host_key_fingerprint
                        && item.pending_start
                })
                .count();
            let pending = pending + usize::from(!was_pending);
            anyhow::ensure!(
                observed_running_jobs.saturating_add(pending as u32) <= max_jobs,
                "remote worker has no unreserved execution slots"
            );
            let expires_at_unix_ms = now.saturating_add(ttl_ms());
            let item = &mut store.reservations[index];
            item.expires_at_unix_ms = expires_at_unix_ms;
            item.pending_start = true;
            self.reservation.expires_at_unix_ms = expires_at_unix_ms;
            self.reservation.pending_start = true;
            Ok(())
        })
    }

    /// Mark the reserved execution as running after the worker accepts start.
    /// Fresh worker probes account for this lease from that point onward.
    pub fn mark_running(&mut self) -> anyhow::Result<()> {
        let id = self.reservation.id.clone();
        with_store(&self.root, |store| {
            let now = now_unix_ms();
            prune(store, now);
            let item = store
                .reservations
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| anyhow::anyhow!("remote worker capacity lease expired"))?;
            anyhow::ensure!(
                item.worker_id == self.reservation.worker_id
                    && item.host_key_fingerprint == self.reservation.host_key_fingerprint,
                "remote worker capacity lease identity changed"
            );
            item.pending_start = false;
            self.reservation.pending_start = false;
            Ok(())
        })
    }

    /// Undo a start reservation only when the worker explicitly rejected the
    /// start before accepting it. Ambiguous transport failures must keep the
    /// pending reservation until its lease expires.
    pub fn rollback_rejected_start(&mut self) -> anyhow::Result<()> {
        self.rollback_rejected_start_at(now_unix_ms())
    }

    fn rollback_rejected_start_at(&mut self, now: u64) -> anyhow::Result<()> {
        let id = self.reservation.id.clone();
        with_store(&self.root, |store| {
            prune(store, now);
            let item = store
                .reservations
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| anyhow::anyhow!("remote worker capacity lease expired"))?;
            anyhow::ensure!(
                item.worker_id == self.reservation.worker_id
                    && item.host_key_fingerprint == self.reservation.host_key_fingerprint,
                "remote worker capacity lease identity changed"
            );
            item.pending_start = false;
            self.reservation.pending_start = false;
            Ok(())
        })
    }

    pub fn release(mut self) -> anyhow::Result<()> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> anyhow::Result<()> {
        let id = self.reservation.id.clone();
        with_store(&self.root, |store| {
            store.reservations.retain(|item| item.id != id);
            Ok(())
        })
    }
}

impl Drop for RemoteWorkerCapacityLease {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

fn prune(store: &mut Store, now: u64) {
    store
        .reservations
        .retain(|item| item.expires_at_unix_ms > now);
}

fn ttl_ms() -> u64 {
    LEASE_TTL.as_millis().min(u64::MAX as u128) as u64
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
#[path = "capacity/tests.rs"]
mod tests;
