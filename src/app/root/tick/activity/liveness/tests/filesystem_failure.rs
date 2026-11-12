use super::*;

/// Production boundary: the liveness sweep reads lifecycle records from the
/// private PlanningStore before deciding whether a stranded TaskIntent needs
/// recovery. This verifies app activity reloads after an unrelated PlanningStore
/// write advances its revision. Dependencies: local file-backed stores only;
/// setup cost is one app fixture and a temporary filesystem fault.
#[test]
fn lifecycle_read_io_failure_is_reported_then_retried() {
    let mut app = crate::app::root::board_tests::fixture();
    let mut root = Work::new(
        "task:liveness-io-failure".into(),
        "A planning goal".into(),
        "Keep the approved goal.".into(),
        "Planning started".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    root.status = WorkStatus::NeedsAttention;
    set_cycle(&mut root);
    let collection = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        project.planning_work = vec![root];
        project.save_planning_work().unwrap();
        project
            .state
            .planning_store
            .root
            .join("state/lifecycle_features")
    };
    std::fs::create_dir_all(collection.parent().unwrap()).unwrap();
    let backup = collection.with_extension("liveness-test-backup");
    let existed = collection.exists();
    if existed {
        std::fs::rename(&collection, &backup).unwrap();
    }
    std::fs::write(&collection, b"not a directory").unwrap();

    app.advance_activity();
    {
        let crate::app::root::Screen::Connected(project) = &app.screen else {
            unreachable!()
        };
        let recovery = project
            .planning_work
            .iter()
            .find(|work| work.key.starts_with("task:manager-liveness:"))
            .expect("unreadable lifecycle state creates a durable recovery action");
        assert_eq!(recovery.status, WorkStatus::InProgress);
        assert!(recovery.detail.contains("could not be inspected"));
        assert_eq!(recovery.liveness_recovery_attempts, 1);
        assert_eq!(
            project.active_planning_work.as_deref(),
            Some(recovery.key.as_str())
        );
        assert!(
            project.active_turn.is_some(),
            "the durable blocker is dispatched"
        );
        let reloaded = crate::core::planning_work::load(&project.state.planning_store).unwrap();
        assert_eq!(
            reloaded.len(),
            2,
            "the blocker survives a planning-store reload"
        );
        assert!(reloaded.iter().any(|work| work.uid == recovery.uid));
        assert!(project.activity.pending.iter().any(|event| {
            event.starts_with("TaskIntent liveness could not inspect lifecycle state")
        }));
        let unrelated = Work::new(
            "task:revision-drift-probe".into(),
            "Unrelated planning record".into(),
            "Advance the shared PlanningStore revision.".into(),
            "Waiting for independent work.".into(),
        );
        let mut externally_saved = project.planning_work.clone();
        externally_saved.push(unrelated);
        crate::core::planning_work::save(&project.state.planning_store, &externally_saved).unwrap();
    }

    std::fs::remove_file(&collection).unwrap();
    if existed {
        std::fs::rename(&backup, &collection).unwrap();
    }
    app.advance_activity();
    let crate::app::root::Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    assert!(
        project
            .planning_work
            .iter()
            .any(|work| work.key.starts_with("task:manager-liveness:"))
    );
    assert!(
        project
            .planning_work
            .iter()
            .any(|work| work.key == "task:revision-drift-probe")
    );
}

#[test]
fn unreadable_lifecycle_state_defers_recovery_while_a_lifecycle_worker_is_active() {
    let mut app = crate::app::root::board_tests::fixture();
    let mut root = Work::new(
        "task:liveness-worker-owner".into(),
        "A goal with a running lifecycle worker".into(),
        "Keep the approved goal.".into(),
        "Planning started".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    set_cycle(&mut root);
    let collection = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        project.planning_work = vec![root];
        project
            .state
            .planning_store
            .root
            .join("state/lifecycle_features")
    };
    std::fs::create_dir_all(collection.parent().unwrap()).unwrap();
    std::fs::write(&collection, b"not a directory").unwrap();
    {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        let (_, result) = std::sync::mpsc::channel();
        let (_, progress) = std::sync::mpsc::channel();
        let (cancel_sender, _cancel_receiver) = tokio::sync::watch::channel(false);
        project.activity.runtime_qa_worker = Some(crate::runtime_qa::QaWorkerHandle::new(
            "fixture-qa-run".into(),
            "fixture-task-uid".into(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cancel_sender,
            progress,
            result,
        ));

        reconcile(project);

        assert_eq!(project.planning_work.len(), 1);
        assert!(project.activity.pending.iter().any(|event| {
            event.contains("deferring recovery because a lifecycle controller is already active")
        }));
    }
    std::fs::remove_file(collection).unwrap();
}

#[test]
fn unreadable_lifecycle_state_defers_recovery_for_a_queued_clarification_planner() {
    let mut app = crate::app::root::board_tests::fixture();
    let mut root = Work::new(
        "task:liveness-planner-owner".into(),
        "A goal with a queued clarification planner".into(),
        "Keep the approved goal.".into(),
        "Planning started".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    set_cycle(&mut root);
    let collection = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        project.planning_work = vec![root];
        project.activity.pending_clarification_planner = Some(("feature-uid".into(), 1));
        project
            .state
            .planning_store
            .root
            .join("state/lifecycle_features")
    };
    std::fs::create_dir_all(collection.parent().unwrap()).unwrap();
    std::fs::write(&collection, b"not a directory").unwrap();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };

    reconcile(project);

    assert_eq!(project.planning_work.len(), 1);
    assert!(project.activity.pending.iter().any(|event| {
        event.contains("deferring recovery because a lifecycle controller is already active")
    }));
    std::fs::remove_file(collection).unwrap();
}

#[test]
fn unreadable_lifecycle_state_requeues_an_orphaned_recovery_within_its_limit() {
    let mut app = crate::app::root::board_tests::fixture();
    let mut root = Work::new(
        "task:liveness-orphaned-recovery".into(),
        "A goal with an interrupted recovery".into(),
        "Keep the approved goal.".into(),
        "Planning started".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    set_cycle(&mut root);
    let recovery = crate::core::planning_work::liveness_recovery_task(
        &root,
        root.task_intent_cycle_uid.as_deref().unwrap(),
        "Prior liveness attempt was interrupted.",
    );
    let recovery_uid = recovery.uid.clone();
    let collection = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        let mut recovery = recovery;
        recovery.status = WorkStatus::InProgress;
        recovery.liveness_recovery_attempts = 1;
        project.planning_work = vec![root, recovery];
        project.save_planning_work().unwrap();
        project
            .state
            .planning_store
            .root
            .join("state/lifecycle_features")
    };
    std::fs::create_dir_all(collection.parent().unwrap()).unwrap();
    std::fs::write(&collection, b"not a directory").unwrap();

    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    project.state.resync().unwrap();
    reconcile(project);
    assert!(
        project.activity.pending.iter().any(|event| {
            event.starts_with("TaskIntent liveness could not inspect lifecycle state")
        }),
        "pending events: {:?}",
        project.activity.pending
    );
    let resumed = project
        .planning_work
        .iter()
        .find(|work| work.uid == recovery_uid)
        .expect("recovery identity remains stable across restart");
    assert_eq!(
        resumed.status,
        WorkStatus::Todo,
        "recovery records: {:?}",
        project
            .planning_work
            .iter()
            .map(|work| (&work.key, &work.uid, work.status))
            .collect::<Vec<_>>()
    );
    assert_eq!(resumed.liveness_recovery_attempts, 1);
    assert!(resumed.detail.contains("could not be inspected"));
    std::fs::remove_file(collection).unwrap();
}
