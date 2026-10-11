use std::{
    fs,
    sync::{Arc, Barrier},
    thread,
};

#[test]
fn persistent_cache_initialization_is_safe_when_called_concurrently() {
    let root = std::env::temp_dir().join(format!(
        "koolade-concurrent-npm-cache-{}",
        uuid::Uuid::new_v4()
    ));
    let task_repository = root.join("repository");
    fs::create_dir_all(&task_repository).unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let workers = (0..8)
        .map(|_| {
            let state_root = root.join("state");
            let task_repository = task_repository.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                super::super::cache::persistent_cache_at(&state_root, &task_repository).unwrap()
            })
        })
        .collect::<Vec<_>>();

    let paths = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert!(paths.windows(2).all(|pair| pair[0] == pair[1]));
    fs::remove_dir_all(root).unwrap();
}
