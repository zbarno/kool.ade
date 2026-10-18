use super::{
    AutoWorkerCapacityCandidate, LEASE_TTL, RemoteWorkerCapacityLease, Reservation, Store, prune,
    with_store,
};
use std::path::Path;

/// Order equal-load workers after the last reserved Auto worker.
/// The cursor shares the capacity store lock and survives controller restarts.
pub fn order_auto_workers(worker_ids: &[String]) -> anyhow::Result<Vec<String>> {
    order_auto_workers_at(&crate::persistence::state_root(), worker_ids)
}

pub(super) fn order_auto_workers_at(
    root: &Path,
    worker_ids: &[String],
) -> anyhow::Result<Vec<String>> {
    let mut ordered = worker_ids.to_vec();
    ordered.sort();
    ordered.dedup();
    let last = with_store(root, |store| Ok(store.last_auto_worker_id.clone()))?;
    if let Some(last) = last {
        let next = ordered
            .iter()
            .position(|id| id.as_str() > last.as_str())
            .unwrap_or(0);
        ordered.rotate_left(next);
    }
    Ok(ordered)
}

/// Persist the worker that actually opened an Auto-routed session. Failed
/// probes and failed connection attempts therefore do not consume a turn.
pub fn record_auto_worker_selection(worker_id: &str) -> anyhow::Result<()> {
    record_auto_worker_selection_at(&crate::persistence::state_root(), worker_id)
}

/// Choose and reserve the next eligible Auto worker under one cross-process
/// lock so concurrent controllers observe each other's reservations.
pub fn reserve_auto_capacity(
    candidates: &[AutoWorkerCapacityCandidate],
) -> anyhow::Result<Option<RemoteWorkerCapacityLease>> {
    reserve_auto_capacity_at(&crate::persistence::state_root(), candidates)
}

pub(super) fn reserve_auto_capacity_at(
    root: &Path,
    candidates: &[AutoWorkerCapacityCandidate],
) -> anyhow::Result<Option<RemoteWorkerCapacityLease>> {
    let now = super::now_unix_ms();
    let reservation = with_store(root, |store| {
        prune(store, now);
        let mut ordered = candidates.to_vec();
        let mut worker_ids = std::collections::HashSet::new();
        anyhow::ensure!(
            ordered
                .iter()
                .all(|candidate| worker_ids.insert(candidate.worker_id.as_str())),
            "Auto worker capacity candidates contain duplicate worker IDs"
        );
        let projected_load = |candidate: &AutoWorkerCapacityCandidate| {
            let pending = store
                .reservations
                .iter()
                .filter(|item| {
                    item.worker_id == candidate.worker_id
                        && item.host_key_fingerprint == candidate.host_key_fingerprint
                        && item.pending_start
                })
                .count() as u32;
            candidate.observed_running_jobs.saturating_add(pending)
        };
        ordered.sort_by(|left, right| {
            projected_load(left)
                .cmp(&projected_load(right))
                .then_with(|| left.worker_id.cmp(&right.worker_id))
        });
        if let Some(last) = &store.last_auto_worker_id {
            for load in ordered
                .iter()
                .map(&projected_load)
                .collect::<std::collections::BTreeSet<_>>()
            {
                let group = ordered
                    .iter()
                    .position(|candidate| projected_load(candidate) == load)
                    .unwrap_or(0);
                let end = ordered[group..]
                    .iter()
                    .position(|candidate| projected_load(candidate) != load)
                    .map_or(ordered.len(), |offset| group + offset);
                let offset = ordered[group..end]
                    .iter()
                    .position(|candidate| candidate.worker_id > *last)
                    .unwrap_or(0);
                ordered[group..end].rotate_left(offset);
            }
        }
        for candidate in ordered {
            anyhow::ensure!(
                !candidate.worker_id.is_empty()
                    && candidate.worker_id.len() <= 128
                    && (1..=256).contains(&candidate.max_jobs)
                    && candidate.host_key_fingerprint.starts_with("SHA256:")
                    && candidate.host_key_fingerprint.len() == 50,
                "Auto worker capacity candidate is invalid"
            );
            if projected_load(&candidate) >= candidate.max_jobs {
                continue;
            }
            let reservation = Reservation {
                id: uuid::Uuid::new_v4().hyphenated().to_string(),
                worker_id: candidate.worker_id.clone(),
                host_key_fingerprint: candidate.host_key_fingerprint,
                expires_at_unix_ms: now.saturating_add(LEASE_TTL.as_millis() as u64),
                pending_start: true,
            };
            store.reservations.push(reservation.clone());
            store.last_auto_worker_id = Some(candidate.worker_id);
            return Ok(Some(reservation));
        }
        Ok(None)
    })?;
    Ok(reservation.map(|reservation| RemoteWorkerCapacityLease {
        root: root.to_path_buf(),
        reservation,
    }))
}

pub(super) fn record_auto_worker_selection_at(root: &Path, worker_id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !worker_id.is_empty() && worker_id.len() <= 128,
        "remote worker ID is invalid"
    );
    with_store(root, |store: &mut Store| {
        store.last_auto_worker_id = Some(worker_id.to_owned());
        Ok(())
    })
}
