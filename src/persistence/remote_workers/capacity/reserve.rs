use std::path::Path;

use super::{RemoteWorkerCapacityLease, Reservation, now_unix_ms, prune, ttl_ms, with_store};

/// Atomically reserves one slot across all controllers using this state root.
/// Worker identity is pinned to the enrolled SSH host key to prevent stale
/// reservations being inherited by a rebound worker ID.
pub fn reserve_capacity(
    worker_id: &str,
    host_key_fingerprint: &str,
    max_jobs: u32,
    observed_running_jobs: u32,
) -> anyhow::Result<RemoteWorkerCapacityLease> {
    reserve_capacity_at(
        &crate::persistence::state_root(),
        worker_id,
        host_key_fingerprint,
        max_jobs,
        observed_running_jobs,
        now_unix_ms(),
    )
}

pub(super) fn reserve_capacity_at(
    root: &Path,
    worker_id: &str,
    host_key_fingerprint: &str,
    max_jobs: u32,
    observed_running_jobs: u32,
    now: u64,
) -> anyhow::Result<RemoteWorkerCapacityLease> {
    anyhow::ensure!(
        !worker_id.is_empty() && worker_id.len() <= 128,
        "remote worker ID is invalid"
    );
    anyhow::ensure!((1..=256).contains(&max_jobs), "worker job limit is invalid");
    anyhow::ensure!(
        host_key_fingerprint.starts_with("SHA256:") && host_key_fingerprint.len() == 50,
        "worker host identity is invalid"
    );
    let id = uuid::Uuid::new_v4().hyphenated().to_string();
    let reservation = Reservation {
        id,
        worker_id: worker_id.to_owned(),
        host_key_fingerprint: host_key_fingerprint.to_owned(),
        expires_at_unix_ms: now.saturating_add(ttl_ms()),
        pending_start: true,
    };
    with_store(root, |store| {
        prune(store, now);
        let active = store
            .reservations
            .iter()
            .filter(|item| {
                item.worker_id == worker_id
                    && item.host_key_fingerprint == host_key_fingerprint
                    && item.pending_start
            })
            .count();
        anyhow::ensure!(
            observed_running_jobs.saturating_add(active as u32) < max_jobs,
            "remote worker has no unreserved execution slots"
        );
        store.reservations.push(reservation.clone());
        Ok(())
    })?;
    Ok(RemoteWorkerCapacityLease {
        root: root.to_path_buf(),
        reservation,
    })
}
