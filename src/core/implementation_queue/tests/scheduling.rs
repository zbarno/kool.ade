use super::*;

#[test]
fn queue_advances_in_order_and_waits_for_dependencies() {
    let docs = vec![
        doc(2, "- [Task 001](001-task.md) must be complete."),
        doc(1, "None."),
    ];
    let mut states = BTreeMap::new();
    assert!(
        ticket_readiness(&docs, &states, &docs[0].path)
            .unwrap_err()
            .contains("waiting")
    );
    assert_eq!(
        next_ticket(&docs, &states).unwrap(),
        Some(docs[1].path.clone())
    );
    states.insert(docs[1].path.clone(), done(&docs[1].path));
    assert!(ticket_readiness(&docs, &states, &docs[0].path).is_ok());
    assert_eq!(
        next_ticket(&docs, &states).unwrap(),
        Some(docs[0].path.clone())
    );
    states.insert(docs[0].path.clone(), done(&docs[0].path));
    assert_eq!(next_ticket(&docs, &states).unwrap(), None);
    assert!(
        next_ticket(&[doc(1, "- [missing](009-task.md)")], &BTreeMap::new())
            .unwrap_err()
            .contains("waiting")
    );
}

#[test]
fn automatic_build_queue_leaves_verified_unpublished_work_for_user_action() {
    let docs = vec![doc(1, "None")];
    let mut state = done(&docs[0].path);
    state.status = crate::core::implementation::ImplementationStatus::ReadyToPublish;
    state.merged_commit = None;
    let states = BTreeMap::from([(docs[0].path.clone(), state)]);

    let waiting = next_ready_ticket(&docs, &states, &Default::default()).unwrap_err();
    assert!(waiting.contains("verified") && waiting.contains("waiting for you to publish"));
}

#[test]
fn independent_tasks_skip_blocked_and_running_predecessors() {
    let docs = vec![
        doc(1, "- [missing](009-task.md)"),
        doc(2, "None."),
        doc(3, "None."),
        doc(4, "- [two](002-task.md)"),
    ];
    let states = BTreeMap::new();
    assert_eq!(
        next_ticket(&docs, &states).unwrap(),
        Some(docs[1].path.clone())
    );
    let active = std::collections::BTreeSet::from([docs[1].path.clone()]);
    assert_eq!(
        next_ready_ticket(&docs, &states, &active).unwrap(),
        Some(docs[2].path.clone())
    );
    let active = std::collections::BTreeSet::from([docs[1].path.clone(), docs[2].path.clone()]);
    assert!(
        next_ready_ticket(&docs, &states, &active)
            .unwrap_err()
            .contains("waiting")
    );
    let states = BTreeMap::from([(docs[1].path.clone(), done(&docs[1].path))]);
    assert_eq!(
        next_ready_ticket(&docs, &states, &active).unwrap(),
        Some(docs[3].path.clone())
    );
}

#[test]
fn cycles_and_invalid_dependencies_do_not_starve_independent_work() {
    let docs = vec![
        doc(1, "- [two](002-task.md)"),
        doc(2, "- [one](001-task.md)"),
        doc(3, "- [unsafe](../other.md)"),
        doc(4, "None."),
    ];
    assert_eq!(
        next_ticket(&docs, &BTreeMap::new()).unwrap(),
        Some(docs[3].path.clone())
    );
    let waiting = next_ticket(&docs[..3], &BTreeMap::new()).unwrap_err();
    assert!(
        waiting.contains("waiting") && waiting.contains("identity"),
        "unexpected dependency explanation: {waiting}"
    );
}

#[test]
fn preferences_and_inflight_ticket_survive_restart_and_lock_excludes_another_window() {
    let root = std::env::temp_dir().join(format!(
        "koolade-queue-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let mut queue = Queue::load(&root).unwrap();
    assert!(queue.auto_plan);
    assert!(queue.auto_build);
    assert!(!queue.auto_publish);
    assert!(!queue.running);
    let lock = Queue::acquire(&root).unwrap();
    assert!(Queue::acquire(&root).is_err());
    let docs = vec![doc(1, "None."), doc(2, "None.")];
    queue.bind_task_documents(&docs).unwrap();
    queue.running = true;
    queue.current_ticket = Some(docs[0].path.clone());
    queue
        .in_flight
        .extend(docs.iter().map(|doc| doc.path.clone()));
    queue.max_parallel = 4;
    queue.save(&root).unwrap();
    drop(lock);
    let _new_lock = Queue::acquire(&root).unwrap();
    let mut loaded = Queue::load(&root).unwrap();
    loaded.bind_task_documents(&docs).unwrap();
    assert!(loaded.running && loaded.auto_build);
    assert!(loaded.auto_plan && !loaded.auto_publish);
    assert_eq!(loaded.current_ticket, queue.current_ticket);
    assert_eq!(loaded.in_flight, queue.in_flight);
    assert_eq!(loaded.max_parallel, 4);
    drop(_new_lock);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn loading_after_a_controller_restart_clears_saved_wall_clock_queue_age() {
    let root = std::env::temp_dir().join(format!(
        "koolade-queue-restart-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let mut queue = Queue::load(&root).unwrap();
    let docs = vec![doc(1, "None.")];
    queue.bind_task_documents(&docs).unwrap();
    assert!(queue.update_ready_waits(&[docs[0].path.clone()], 100, true));
    queue.save(&root).unwrap();

    let mut restored = Queue::load(&root).unwrap();
    restored.bind_task_documents(&docs).unwrap();
    assert_eq!(restored.take_queue_wait_ms(&docs[0].path, 10_000), None);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn queue_load_does_not_rewrite_age_while_another_controller_holds_the_lock() {
    let root = std::env::temp_dir().join(format!(
        "koolade-queue-owned-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let mut queue = Queue::load(&root).unwrap();
    let docs = vec![doc(1, "None.")];
    queue.bind_task_documents(&docs).unwrap();
    assert!(queue.update_ready_waits(&[docs[0].path.clone()], 100, true));
    queue.save(&root).unwrap();

    let active_lock = Queue::acquire(&root).unwrap();
    let mut restored = Queue::load(&root).unwrap();
    let queue_path = directory(&root).unwrap().join("koolade-queue.json");
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&queue_path).unwrap()).unwrap();
    let task_uid = docs[0].identity.as_ref().unwrap().uid.as_str();
    assert_eq!(stored["tasks"][task_uid]["waiting_since_unix_ms"], 100);
    drop(active_lock);
    assert_eq!(restored.take_queue_wait_ms(&docs[0].path, 10_000), None);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn runnable_ticket_collection_returns_all_eligible_work() {
    let docs = vec![doc(3, "None."), doc(1, "None."), doc(2, "None.")];
    let ready = ready_tickets_with_lifecycle_subtasks(
        &docs,
        &BTreeMap::new(),
        &Default::default(),
        &Default::default(),
        &Default::default(),
    );
    assert_eq!(
        ready,
        vec![
            docs[1].path.clone(),
            docs[2].path.clone(),
            docs[0].path.clone()
        ]
    );
}
