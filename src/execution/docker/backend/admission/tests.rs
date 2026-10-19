#[test]
fn unknown_capacity_lease_is_shared_across_independent_backend_instances() {
    let root = std::env::temp_dir().join(format!("koolade-capacity-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let first_pool =
        crate::execution::remote::WorkerCapacityPool::new_local_unknown(&root).unwrap();
    let second_pool =
        crate::execution::remote::WorkerCapacityPool::new_local_unknown(&root).unwrap();
    let first = first_pool.acquire_unknown_local().unwrap();
    assert!(second_pool.acquire_unknown_local().is_err());
    drop(first);
    let second = second_pool.acquire_unknown_local().unwrap();
    drop(second);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn known_and_unknown_local_capacity_leases_exclude_each_other() {
    let root = std::env::temp_dir().join(format!("koolade-capacity-{}", uuid::Uuid::new_v4()));
    let known_pool = crate::execution::remote::WorkerCapacityPool::new_local(&root).unwrap();
    let unknown_pool =
        crate::execution::remote::WorkerCapacityPool::new_local_unknown(&root).unwrap();
    let known_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let known = known_pool
        .acquire_resources(&known_id, 500, 500, 1_000, 1_000)
        .unwrap();
    assert!(unknown_pool.acquire_unknown_local().is_err());
    drop(known);

    let unknown = unknown_pool.acquire_unknown_local().unwrap();
    let second_id = uuid::Uuid::new_v4().hyphenated().to_string();
    assert!(
        known_pool
            .acquire_resources(&second_id, 100, 100, 1_000, 1_000)
            .is_err()
    );
    drop(unknown);
    std::fs::remove_dir_all(root).unwrap();
}
