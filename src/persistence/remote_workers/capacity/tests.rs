use super::{reserve_capacity_at, with_store};
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
fn expired_crashed_controller_lease_is_reclaimed_and_identity_isolated() {
    let root = tempdir().unwrap();
    with_store(root.path(), |store| {
        store.reservations.push(super::Reservation {
            id: "crashed-owner".into(),
            worker_id: WORKER.into(),
            host_key_fingerprint: KEY.into(),
            expires_at_unix_ms: 100,
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
