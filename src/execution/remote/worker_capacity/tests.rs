use super::{MAX_REMOTE_EXECUTIONS, WorkerCapacityPool};
use std::{
    sync::{Arc, Barrier},
    thread,
};

#[test]
fn worker_slots_are_global_across_independent_controller_pools() {
    let root = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let root_path = root.path().to_path_buf();
    let barrier_a = barrier.clone();
    let a = thread::spawn(move || {
        let pool =
            WorkerCapacityPool::new(&root_path, "00000000-0000-4000-8000-000000000001".into())
                .unwrap();
        let mut leases = Vec::new();
        for _ in 0..MAX_REMOTE_EXECUTIONS - 1 {
            leases.push(pool.acquire().unwrap());
        }
        barrier_a.wait();
        let acquired = pool.acquire().ok();
        barrier_a.wait();
        let result = acquired.is_some();
        drop(leases);
        drop(acquired);
        result
    });
    let root_path = root.path().to_path_buf();
    let barrier_b = barrier.clone();
    let b = thread::spawn(move || {
        let pool =
            WorkerCapacityPool::new(&root_path, "00000000-0000-4000-8000-000000000001".into())
                .unwrap();
        barrier_b.wait();
        let acquired = pool.acquire().ok();
        barrier_b.wait();
        acquired.is_some()
    });
    let admitted = [a.join().unwrap(), b.join().unwrap()]
        .into_iter()
        .filter(|was_admitted| *was_admitted)
        .count();
    assert_eq!(admitted, 1);
}

#[test]
fn worker_slot_is_released_on_disconnect_and_isolated_by_worker_identity() {
    let root = tempfile::tempdir().unwrap();
    let pool = WorkerCapacityPool::new(root.path(), "00000000-0000-4000-8000-000000000001".into())
        .unwrap();
    let lease = pool.acquire().unwrap();
    drop(lease);
    assert!(pool.acquire().is_ok());
    assert!(
        WorkerCapacityPool::new(root.path(), "00000000-0000-4000-8000-000000000002".into())
            .unwrap()
            .acquire()
            .is_ok()
    );
}
