use super::*;

#[test]
fn future_history_schema_is_not_replaced() {
    let root = std::env::temp_dir().join(format!("koolade-recency-{}", uuid::Uuid::new_v4()));
    let manager = WorkspaceManager::new(&root);
    let id = uuid::Uuid::new_v4();
    fs::create_dir_all(manager.project_dir(id)).unwrap();
    fs::write(manager.association_path(id), b"association sentinel").unwrap();
    let bytes = br#"{"version":2,"opened":{}}"#;
    fs::write(root.join("workspace-recency.json"), bytes).unwrap();
    assert!(manager.record_successful_open(id).is_err());
    assert_eq!(
        fs::read(root.join("workspace-recency.json")).unwrap(),
        bytes
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn damaged_history_is_preserved_and_concurrent_opens_are_recorded() {
    // Boundary: real filesystem locking across concurrent writers; no
    // external services. A pure sorting test cannot prove serialization.
    let root = std::env::temp_dir().join(format!("koolade-recency-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("workspace-recency.json"), b"damaged history").unwrap();
    let ids: Vec<_> = (0..8).map(|_| uuid::Uuid::new_v4()).collect();
    let manager = WorkspaceManager::new(&root);
    for id in &ids {
        fs::create_dir_all(manager.project_dir(*id)).unwrap();
        fs::write(manager.association_path(*id), b"association sentinel").unwrap();
    }
    std::thread::scope(|scope| {
        for id in &ids {
            let manager = &manager;
            scope.spawn(move || manager.record_successful_open(*id).unwrap());
        }
    });
    assert_eq!(manager.last_opened_dates().unwrap().len(), ids.len());
    let damaged: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("workspace-recency-damaged-")
        })
        .collect();
    assert_eq!(damaged.len(), 1);
    assert_eq!(fs::read(damaged[0].path()).unwrap(), b"damaged history");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn successful_open_persists_order_without_changing_shared_data() {
    // Boundary: private disk history across manager instances. No Git,
    // network or Docker dependency; temporary-directory setup only.
    // Unit sorting alone cannot establish durable ordering or isolation.
    let root = std::env::temp_dir().join(format!("koolade-recency-{}", uuid::Uuid::new_v4()));
    let manager = WorkspaceManager::new(&root);
    let first = uuid::Uuid::new_v4();
    let second = uuid::Uuid::new_v4();
    for id in [first, second] {
        fs::create_dir_all(manager.project_dir(id)).unwrap();
        fs::write(manager.association_path(id), b"association sentinel").unwrap();
    }
    let mut items = vec![
        LocalWorkspaceSummary {
            id: first,
            title: "Alpha".into(),
            planning_repo: root.join("a"),
        },
        LocalWorkspaceSummary {
            id: second,
            title: "Beta".into(),
            planning_repo: root.join("b"),
        },
    ];
    manager.sort_by_recent_open(&mut items).unwrap();
    assert_eq!(items[0].id, first);
    manager.record_successful_open(second).unwrap();
    let reopened = WorkspaceManager::new(&root);
    reopened.sort_by_recent_open(&mut items).unwrap();
    assert_eq!(items[0].id, second);
    let before = fs::read(root.join("workspace-recency.json")).unwrap();
    reopened.sort_by_recent_open(&mut items).unwrap();
    assert_eq!(
        before,
        fs::read(root.join("workspace-recency.json")).unwrap()
    );
    assert_eq!(
        fs::read(manager.association_path(second)).unwrap(),
        b"association sentinel"
    );
    assert!(
        manager
            .record_successful_open(uuid::Uuid::new_v4())
            .is_err()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn queued_open_survives_restart_and_late_writers_preserve_order() {
    // Real private event/history storage across manager lifetimes. No Git or
    // services; two association sentinels. Unit sorting cannot prove recovery.
    let root = std::env::temp_dir().join(format!("koolade-pending-{}", uuid::Uuid::new_v4()));
    let manager = WorkspaceManager::new(&root);
    let first = uuid::Uuid::new_v4();
    let second = uuid::Uuid::new_v4();
    for id in [first, second] {
        fs::create_dir_all(manager.project_dir(id)).unwrap();
        fs::write(manager.association_path(id), b"association sentinel").unwrap();
    }
    manager.queue_successful_open(first).unwrap();
    assert!(manager.last_opened_dates().unwrap().is_empty());
    drop(manager); // Simulate shutdown before the worker starts.
    let manager = WorkspaceManager::new(&root);
    manager.flush_pending_opens().unwrap();
    let date = manager.last_opened_dates().unwrap()[&first];
    manager
        .record_successful_open_at(second, date + chrono::Duration::seconds(1))
        .unwrap();
    manager
        .record_successful_open_at(first, date - chrono::Duration::seconds(1))
        .unwrap();
    let dates = manager.last_opened_dates().unwrap();
    assert_eq!(dates[&first], date);
    assert!(dates[&second] > dates[&first]);
    manager.flush_pending_opens().unwrap();
    assert_eq!(dates, manager.last_opened_dates().unwrap());
    assert_eq!(
        fs::read_dir(root.join("pending-workspace-opens"))
            .unwrap()
            .count(),
        0
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn pending_event_is_retained_on_failure_and_concurrent_retries_are_safe() {
    // Filesystem failure/retry and concurrent drain boundary, temporary files
    // only. Pure serialization cannot prove event retention or idempotence.
    let root = std::env::temp_dir().join(format!("koolade-pending-retry-{}", uuid::Uuid::new_v4()));
    let manager = WorkspaceManager::new(&root);
    let id = uuid::Uuid::new_v4();
    fs::create_dir_all(manager.project_dir(id)).unwrap();
    fs::write(manager.association_path(id), b"association sentinel").unwrap();
    manager.queue_successful_open(id).unwrap();
    let history_path = root.join("workspace-recency.json");
    fs::create_dir(&history_path).unwrap();
    assert!(manager.flush_pending_opens().is_err());
    assert_eq!(
        fs::read_dir(root.join("pending-workspace-opens"))
            .unwrap()
            .count(),
        1
    );
    fs::remove_dir(history_path).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| manager.flush_pending_opens().unwrap());
        }
    });
    assert!(manager.last_opened_dates().unwrap().contains_key(&id));
    assert_eq!(
        fs::read_dir(root.join("pending-workspace-opens"))
            .unwrap()
            .count(),
        0
    );
    fs::remove_dir_all(root).unwrap();
}
