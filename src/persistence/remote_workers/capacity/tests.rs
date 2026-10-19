use super::{AutoWorkerCapacityCandidate, reserve_capacity_at, scheduler_cursor::*, with_store};
use std::{
    sync::{Arc, Barrier},
    thread,
};
use tempfile::tempdir;

const WORKER: &str = "worker-a";
const KEY: &str = "SHA256:abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQ";

#[test]
fn independent_store_handles_cannot_oversubscribe_worker() {
    let root = Arc::new(tempdir().unwrap().keep());
    let barrier = Arc::new(Barrier::new(2));
    let attempts = (0..2)
        .map(|_| {
            let root = root.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                reserve_capacity_at(&root, WORKER, KEY, 1, 0, 100)
            })
        })
        .collect::<Vec<_>>();
    let reservations = attempts
        .into_iter()
        .map(|attempt| attempt.join().unwrap())
        .collect::<Vec<_>>();
    let admitted = reservations.iter().filter(|lease| lease.is_ok()).count();
    assert_eq!(admitted, 1);
}

#[test]
fn release_allows_next_controller_to_reserve() {
    let root = tempdir().unwrap();
    let lease = reserve_capacity_at(root.path(), WORKER, KEY, 1, 0, 100).unwrap();
    assert!(reserve_capacity_at(root.path(), WORKER, KEY, 1, 0, 101).is_err());
    lease.release().unwrap();
    assert!(reserve_capacity_at(root.path(), WORKER, KEY, 1, 0, 102).is_ok());
}

#[test]
fn rejected_worker_start_releases_pending_capacity_without_releasing_session_lease() {
    let root = tempdir().unwrap();
    let mut lease = reserve_capacity_at(root.path(), WORKER, KEY, 1, 0, 100).unwrap();
    assert!(reserve_capacity_at(root.path(), WORKER, KEY, 1, 0, 101).is_err());

    lease.rollback_rejected_start_at(102).unwrap();

    assert!(reserve_capacity_at(root.path(), WORKER, KEY, 1, 0, 103).is_ok());
    assert!(lease.prepare_start(1, 0).is_err());
}

#[test]
fn expired_crashed_controller_lease_is_reclaimed_and_identity_isolated() {
    let root = tempdir().unwrap();
    with_store(root.path(), |store| {
        store.reservations.push(super::Reservation {
            id: "crashed-owner".into(),
            worker_id: WORKER.into(),
            host_key_fingerprint: KEY.into(),
            expires_at_unix_ms: 100,
            pending_start: true,
        });
        Ok(())
    })
    .unwrap();
    assert!(reserve_capacity_at(root.path(), WORKER, KEY, 1, 0, 101).is_ok());
    assert!(
        reserve_capacity_at(
            root.path(),
            WORKER,
            "SHA256:BCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqr",
            1,
            0,
            101
        )
        .is_ok()
    );
}

#[test]
fn malformed_or_symlinked_store_is_not_overwritten() {
    let root = tempdir().unwrap();
    let path = root.path().join(super::STORE);
    std::fs::write(&path, b"not json").unwrap();
    assert!(reserve_capacity_at(root.path(), WORKER, KEY, 1, 0, 100).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"not json");
}

#[test]
fn auto_worker_rotation_is_durable_and_skips_workers_not_in_the_eligible_set() {
    let root = tempdir().unwrap();
    let workers = vec!["worker-c".into(), "worker-a".into(), "worker-b".into()];
    assert_eq!(
        order_auto_workers_at(root.path(), &workers).unwrap(),
        vec!["worker-a", "worker-b", "worker-c"]
    );

    record_auto_worker_selection_at(root.path(), "worker-b").unwrap();
    assert_eq!(
        order_auto_workers_at(root.path(), &workers).unwrap(),
        vec!["worker-c", "worker-a", "worker-b"]
    );
    assert_eq!(
        order_auto_workers_at(root.path(), &["worker-c".into(), "worker-a".into()]).unwrap(),
        vec!["worker-c", "worker-a"]
    );
}

#[test]
fn auto_worker_rotation_reads_capacity_stores_without_a_cursor() {
    let root = tempdir().unwrap();
    std::fs::write(
        root.path().join(super::STORE),
        br#"{"schema_version":1,"reservations":[]}"#,
    )
    .unwrap();
    assert_eq!(
        order_auto_workers_at(root.path(), &["worker-b".into(), "worker-a".into()]).unwrap(),
        vec!["worker-a", "worker-b"]
    );
}

#[test]
fn concurrent_auto_reservations_choose_distinct_equal_load_workers() {
    let root = Arc::new(tempdir().unwrap().keep());
    let candidates = vec!["worker-a", "worker-b"]
        .into_iter()
        .map(|worker_id| AutoWorkerCapacityCandidate {
            worker_id: worker_id.into(),
            host_key_fingerprint: KEY.into(),
            max_jobs: 1,
            observed_running_jobs: 0,
        })
        .collect::<Vec<_>>();
    let barrier = Arc::new(Barrier::new(2));
    let attempts = (0..2)
        .map(|_| {
            let root = root.clone();
            let candidates = candidates.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                reserve_auto_capacity_at(&root, &candidates)
                    .unwrap()
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let leases = attempts
        .into_iter()
        .map(|attempt| attempt.join().unwrap())
        .collect::<Vec<_>>();
    assert_ne!(leases[0].worker_id(), leases[1].worker_id());
}

#[test]
fn auto_reservations_balance_projected_load_from_unequal_worker_baselines() {
    let root = tempfile::tempdir().unwrap();
    let candidates = [
        AutoWorkerCapacityCandidate {
            worker_id: "worker-a".into(),
            host_key_fingerprint: KEY.into(),
            max_jobs: 4,
            observed_running_jobs: 0,
        },
        AutoWorkerCapacityCandidate {
            worker_id: "worker-b".into(),
            host_key_fingerprint: KEY.into(),
            max_jobs: 4,
            observed_running_jobs: 1,
        },
    ];

    let first = reserve_auto_capacity_at(root.path(), &candidates)
        .unwrap()
        .unwrap();
    let second = reserve_auto_capacity_at(root.path(), &candidates)
        .unwrap()
        .unwrap();

    assert_eq!(first.worker_id(), "worker-a");
    assert_eq!(second.worker_id(), "worker-b");
}

#[test]
fn auto_reservations_revalidate_a_lowered_capacity_limit() {
    let root = tempdir().unwrap();
    let candidates = [AutoWorkerCapacityCandidate {
        worker_id: WORKER.into(),
        host_key_fingerprint: KEY.into(),
        max_jobs: 2,
        observed_running_jobs: 0,
    }];
    let first = reserve_auto_capacity_at(root.path(), &candidates)
        .unwrap()
        .unwrap();
    let second = reserve_auto_capacity_at(root.path(), &candidates)
        .unwrap()
        .unwrap();
    assert!(first.revalidate(1, 0).is_err());
    first.release().unwrap();
    second.revalidate(1, 0).unwrap();
}

#[test]
fn running_auto_lease_is_not_double_counted_with_fresh_worker_load() {
    let root = tempdir().unwrap();
    let candidate = [AutoWorkerCapacityCandidate {
        worker_id: WORKER.into(),
        host_key_fingerprint: KEY.into(),
        max_jobs: 2,
        observed_running_jobs: 0,
    }];
    let mut running = reserve_auto_capacity_at(root.path(), &candidate)
        .unwrap()
        .unwrap();
    running.mark_running().unwrap();
    running.renew().unwrap();
    let second = [AutoWorkerCapacityCandidate {
        observed_running_jobs: 1,
        ..candidate[0].clone()
    }];
    let second = reserve_auto_capacity_at(root.path(), &second)
        .unwrap()
        .unwrap();
    second.release().unwrap();
    running.prepare_start(2, 0).unwrap();
    let idle = [AutoWorkerCapacityCandidate {
        observed_running_jobs: 0,
        ..candidate[0].clone()
    }];
    let pending = reserve_auto_capacity_at(root.path(), &idle)
        .unwrap()
        .unwrap();
    assert!(
        reserve_auto_capacity_at(root.path(), &idle)
            .unwrap()
            .is_none()
    );
    drop(pending);
}

#[test]
fn concurrent_idle_sessions_cannot_reactivate_above_a_lowered_limit() {
    let root = Arc::new(tempdir().unwrap().keep());
    let candidate = [AutoWorkerCapacityCandidate {
        worker_id: WORKER.into(),
        host_key_fingerprint: KEY.into(),
        max_jobs: 2,
        observed_running_jobs: 0,
    }];
    let mut leases = (0..2)
        .map(|_| {
            reserve_auto_capacity_at(&root, &candidate)
                .unwrap()
                .unwrap()
        })
        .collect::<Vec<_>>();
    for lease in &mut leases {
        lease.mark_running().unwrap();
    }
    let barrier = Arc::new(Barrier::new(2));
    let attempts = leases
        .into_iter()
        .map(|mut lease| {
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                let admitted = lease.prepare_start(1, 0).is_ok();
                (lease, admitted)
            })
        })
        .collect::<Vec<_>>();
    let reservations = attempts
        .into_iter()
        .map(|attempt| attempt.join().unwrap())
        .collect::<Vec<_>>();
    let admitted = reservations
        .iter()
        .filter(|(_, admitted)| *admitted)
        .count();
    assert_eq!(admitted, 1);
}
