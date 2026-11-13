use crate::app::root::{Screen, board_tests};
use crate::core::planning_work::{Work, WorkStatus};
use crate::domain::{ChatRole, ManagerEventCheckpointV1, ManagerRunEventV1, ManagerRunOutcomeV1};

/// Production boundary: ordinary app ticks detect a malformed Manager event
/// stream while the independent TaskIntent liveness sweep still creates one
/// durable recovery action and preserves it across repeated ticks. Separate
/// Manager and liveness tests cannot prove the app composes the two paths.
/// Uses synthetic private stores only; no provider, Docker, SSH, or network;
/// moderate setup.
#[test]
fn blocked_manager_replay_does_not_suppress_taskintent_liveness_recovery() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = std::env::temp_dir().join(format!("manager-liveness-home-{}", std::process::id()));
    environment.set("KOOLADE_HOME", &home);
    let slug = format!("manager-liveness-{}", uuid::Uuid::new_v4());
    let checkpoint = ManagerEventCheckpointV1::from_messages(
        &["Inspect the stranded approved TaskIntent".into()],
        None,
        None,
        None,
    )
    .unwrap();
    let run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let event = |sequence, outcome| ManagerRunEventV1 {
        run_id: run_id.clone(),
        sequence,
        outcome,
        checkpoint: (sequence == 1).then(|| checkpoint.clone()),
        checkpoint_cursor: checkpoint.cursor,
        checkpoint_sha256: checkpoint.sha256.clone(),
        retry_after_unix_ms: None,
    };
    let mut queued = crate::domain::ChatMessage::new(ChatRole::System, "Manager queued", None);
    queued.manager_run_event = Some(event(1, ManagerRunOutcomeV1::Queued));
    let mut completed =
        crate::domain::ChatMessage::new(ChatRole::System, "Manager completed", None);
    completed.manager_run_event = Some(event(3, ManagerRunOutcomeV1::Completed));
    let mut started = crate::domain::ChatMessage::new(ChatRole::System, "Manager started", None);
    started.manager_run_event = Some(event(2, ManagerRunOutcomeV1::Started));
    crate::persistence::manager_activity::append(&slug, &[queued, completed, started]).unwrap();

    let mut app = board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    project.chat_slug = slug.clone();
    project.activity.manager_history = crate::persistence::manager_activity::load(&slug);
    let mut root = Work::new(
        "task:manager-liveness-root".into(),
        "Continue the approved goal".into(),
        "Inspect why this approved TaskIntent has no eligible operation.".into(),
        "The work must remain visible and recoverable.".into(),
    );
    root.status = WorkStatus::InProgress;
    root.task_intent_uid = Some(root.uid.clone());
    let cycle_uid = root.uid.clone();
    root.task_intent_cycles = vec![crate::core::planning_work::TaskIntentCycleV1 {
        uid: cycle_uid.clone(),
        sequence: 1,
        goal: root.request.clone(),
        work_uid: root.uid.clone(),
        started_at_ms: None,
    }];
    root.task_intent_cycle_uid = Some(cycle_uid.clone());
    let root_uid = root.uid.clone();
    project.planning_work = vec![root];
    project.active_planning_work = Some("unrelated-work-in-flight".into());
    project.save_planning_work().unwrap();

    app.advance_activity();
    let recovery_uid = {
        let Screen::Connected(project) = &app.screen else {
            unreachable!()
        };
        assert!(
            project
                .activity
                .manager_history_error
                .as_deref()
                .is_some_and(|error| error.contains("sequence 3"))
        );
        let recoveries = project
            .planning_work
            .iter()
            .filter(|work| work.key.starts_with("task:manager-liveness:"))
            .collect::<Vec<_>>();
        assert_eq!(recoveries.len(), 1);
        let recovery = recoveries[0];
        assert_eq!(recovery.status, WorkStatus::Todo);
        assert!(
            recovery.parent_uid.as_deref() == Some(root_uid.as_str()),
            "recovery action belongs to the stranded TaskIntent"
        );
        assert!(
            recovery.task_intent_uid.as_deref() == Some(root_uid.as_str()),
            "recovery action preserves the TaskIntent UID"
        );
        assert!(
            recovery.task_intent_cycle_uid.as_deref() == Some(cycle_uid.as_str()),
            "recovery action preserves the active cycle UID"
        );
        recovery.uid.clone()
    };

    app.advance_activity();
    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    let recoveries = project
        .planning_work
        .iter()
        .filter(|work| work.key.starts_with("task:manager-liveness:"))
        .collect::<Vec<_>>();
    assert!(
        recoveries.len() == 1 && recoveries[0].uid == recovery_uid,
        "repeated app ticks preserve one exact recovery identity"
    );
    let persisted = crate::core::planning_work::load(&project.state.planning_store).unwrap();
    assert_eq!(
        persisted
            .iter()
            .filter(|work| work.key.starts_with("task:manager-liveness:"))
            .count(),
        1,
        "reload preserves exactly one durable recovery action"
    );
    assert!(
        persisted.iter().any(|work| work.uid == recovery_uid),
        "the original recovery UID survives reload"
    );
    assert!(
        persisted.iter().any(|work| work.uid == root_uid),
        "the TaskIntent root survives reload"
    );
}
