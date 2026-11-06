use super::*;
use std::{process::Command, sync::Arc};

#[test]
fn floor_fails_closed_for_missing_corrupt_and_hardlinked_state() {
    // Production boundary: persistent anti-rollback state must never reset on
    // deletion, corruption, or unsafe inode aliasing. Local filesystem only;
    // no VM, Docker, network, or SSH; low setup cost.
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("app-state");
    fs::create_dir(&parent).unwrap();
    let missing = parent.join("missing-floor");
    assert!(matches!(
        GuestImageCatalogGenerationFloorV1::open(missing),
        Err(ImageCacheErrorV1::CatalogFloorUnavailable)
    ));

    let corrupt_root = parent.join("corrupt-floor");
    let corrupt =
        GuestImageCatalogGenerationFloorV1::initialize_for_first_install(corrupt_root.clone(), 1)
            .unwrap();
    drop(corrupt);
    fs::write(corrupt_root.join(STATE_FILE), b"malformed\n").unwrap();
    assert!(matches!(
        GuestImageCatalogGenerationFloorV1::open(corrupt_root),
        Err(ImageCacheErrorV1::CatalogFloorUnavailable)
    ));

    let linked_root = parent.join("linked-floor");
    let linked =
        GuestImageCatalogGenerationFloorV1::initialize_for_first_install(linked_root.clone(), 1)
            .unwrap();
    drop(linked);
    fs::hard_link(linked_root.join(STATE_FILE), parent.join("state-hardlink")).unwrap();
    assert!(matches!(
        GuestImageCatalogGenerationFloorV1::open(linked_root),
        Err(ImageCacheErrorV1::CatalogFloorUnavailable)
    ));

    let linked_lock_root = parent.join("linked-lock-floor");
    let linked_lock = GuestImageCatalogGenerationFloorV1::initialize_for_first_install(
        linked_lock_root.clone(),
        1,
    )
    .unwrap();
    drop(linked_lock);
    fs::hard_link(
        linked_lock_root.join(LOCK_FILE),
        parent.join("lock-hardlink"),
    )
    .unwrap();
    assert!(matches!(
        GuestImageCatalogGenerationFloorV1::open(linked_lock_root),
        Err(ImageCacheErrorV1::CatalogFloorUnavailable)
    ));
}

#[test]
fn concurrent_updates_serialize_and_never_lower_the_floor() {
    // Production boundary: competing host launch requests share one durable
    // generation floor. This exercises inter-thread file locking, not a parser
    // alone; local disk only; no VM/network; low setup cost.
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app-state/catalog-floor");
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    let floor =
        GuestImageCatalogGenerationFloorV1::initialize_for_first_install(root.clone(), 1).unwrap();
    let shared = Arc::new(floor);
    let workers: Vec<_> = (2..=16)
        .map(|generation| {
            let floor = Arc::clone(&shared);
            std::thread::spawn(move || floor.accept_generation(generation))
        })
        .collect();
    for worker in workers {
        assert!(matches!(
            worker.join().unwrap(),
            Ok(()) | Err(ImageCacheErrorV1::CatalogRollback)
        ));
    }
    drop(shared);
    let reopened = GuestImageCatalogGenerationFloorV1::open(root).unwrap();
    assert_eq!(reopened.accept_generation(16), Ok(()));
    assert_eq!(
        reopened.accept_generation(1),
        Err(ImageCacheErrorV1::CatalogRollback)
    );
}

#[test]
fn subprocesses_serialize_catalog_floor_updates() {
    // Production boundary: independent application launches must coordinate on
    // the same durable generation floor. Test-harness subprocesses concurrently
    // update local state; no VM, Docker, network, or SSH; low setup cost.
    const ROOT_ENV: &str = "KOOLADE_TEST_CATALOG_FLOOR_ROOT";
    const GENERATION_ENV: &str = "KOOLADE_TEST_CATALOG_FLOOR_GENERATION";
    if let (Ok(root), Ok(generation)) = (std::env::var(ROOT_ENV), std::env::var(GENERATION_ENV)) {
        let floor = GuestImageCatalogGenerationFloorV1::open(root.into()).unwrap();
        let result = floor.accept_generation(generation.parse().unwrap());
        assert!(matches!(
            result,
            Ok(()) | Err(ImageCacheErrorV1::CatalogRollback)
        ));
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("app-state");
    fs::create_dir(&parent).unwrap();
    let root = parent.join("catalog-floor");
    GuestImageCatalogGenerationFloorV1::initialize_for_first_install(root.clone(), 1).unwrap();
    let executable = std::env::current_exe().unwrap();
    let listing = Command::new(&executable).arg("--list").output().unwrap();
    assert!(listing.status.success());
    let listing = String::from_utf8(listing.stdout).unwrap();
    let worker_name = listing
        .lines()
        .find_map(|line| {
            line.strip_suffix(": test")
                .filter(|name| name.ends_with("subprocesses_serialize_catalog_floor_updates"))
        })
        .unwrap();
    let children: Vec<_> = (2..=12)
        .map(|generation| {
            Command::new(&executable)
                .arg("--exact")
                .arg(worker_name)
                .env(ROOT_ENV, &root)
                .env(GENERATION_ENV, generation.to_string())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "subprocess failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let floor = GuestImageCatalogGenerationFloorV1::open(root).unwrap();
    let lock = floor.lock(false).unwrap();
    assert_eq!(floor.read_generation().unwrap(), 12);
    FileExt::unlock(&lock).unwrap();
}
