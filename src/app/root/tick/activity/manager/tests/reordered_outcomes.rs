use super::*;

/// Production boundary: ordinary app recovery reloads an out-of-order Manager
/// event stream, refuses to replay its checkpoint, and keeps the exact saved
/// source event and existing TaskIntent recovery action available after another
/// app restart. A sequence validator unit test cannot prove persistence and
/// restart behavior together. Uses synthetic private stores only; no external
/// services; moderate setup.
#[test]
fn restart_blocks_reordered_manager_outcomes_without_losing_checkpoint() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = std::env::temp_dir().join(format!("manager-reorder-home-{}", std::process::id()));
    environment.set("KOOLADE_HOME", &home);
    let slug = format!("manager-reorder-{}", uuid::Uuid::new_v4());
    let checkpoint = crate::domain::ManagerEventCheckpointV1::from_messages(
        &["Investigate the saved TaskIntent blocker".into()],
        None,
        None,
        None,
    )
    .unwrap();
    let run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let event = |sequence, outcome| crate::domain::ManagerRunEventV1 {
        run_id: run_id.clone(),
        sequence,
        outcome,
        checkpoint: (sequence == 1).then(|| checkpoint.clone()),
        checkpoint_cursor: checkpoint.cursor,
        checkpoint_sha256: checkpoint.sha256.clone(),
        retry_after_unix_ms: None,
    };
    let mut queued = ChatMessage::new(ChatRole::System, "Manager queued", None);
    queued.manager_run_event = Some(event(1, crate::domain::ManagerRunOutcomeV1::Queued));
    let mut completed = ChatMessage::new(ChatRole::System, "Manager completed", None);
    completed.manager_run_event = Some(event(3, crate::domain::ManagerRunOutcomeV1::Completed));
    let mut started = ChatMessage::new(ChatRole::System, "Manager started", None);
    started.manager_run_event = Some(event(2, crate::domain::ManagerRunOutcomeV1::Started));
    crate::persistence::manager_activity::append(&slug, &[queued, completed, started]).unwrap();

    let mut app = crate::app::root::board_tests::fixture();
    {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        project.chat_slug = slug.clone();
        project.activity.manager_history = crate::persistence::manager_activity::load(&slug);
        let mut root = crate::core::planning_work::Work::new(
            "task:manager-reorder-root".into(),
            "Continue the approved goal".into(),
            "Keep the existing TaskIntent action available.".into(),
            "Manager supervision was interrupted.".into(),
        );
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
        let mut action = crate::core::planning_work::Work::new(
            "task:manager-reorder-action".into(),
            "Review the interrupted Manager event".into(),
            "Resolve the preserved supervision checkpoint.".into(),
            "Manager outcomes are out of order.".into(),
        );
        action.status = crate::core::planning_work::WorkStatus::NeedsAttention;
        action.parent_uid = Some(root.uid.clone());
        action.task_intent_uid = Some(root.uid.clone());
        action.task_intent_cycle_uid = Some(cycle_uid);
        project.planning_work = vec![root, action];
        project.save_planning_work().unwrap();
    }
    app.advance_activity();
    let first_error = match &app.screen {
        crate::app::root::Screen::Connected(project) => project
            .activity
            .manager_history_error
            .clone()
            .expect("reordered Manager outcomes block replay"),
        _ => unreachable!(),
    };
    assert!(first_error.contains("sequence 3"));
    let action_uid = match &app.screen {
        crate::app::root::Screen::Connected(project) => {
            let action = project
                .planning_work
                .iter()
                .find(|work| work.key == "task:manager-reorder-action")
                .expect("reordered Manager events retain an actionable blocker");
            assert_eq!(
                action.status,
                crate::core::planning_work::WorkStatus::NeedsAttention
            );
            action.uid.clone()
        }
        _ => unreachable!(),
    };
    assert!(
        matches!(&app.screen, crate::app::root::Screen::Connected(project)
        if project.activity.pending.iter().all(|event| !event.contains("Investigate the saved TaskIntent blocker")))
    );
    let before_restart = crate::persistence::manager_activity::load(&slug);
    assert!(before_restart.iter().any(|message| {
        message.manager_run_event.as_ref().is_some_and(|event| {
            event.sequence == 1 && event.checkpoint.as_ref() == Some(&checkpoint)
        })
    }));

    let mut restarted = crate::app::root::board_tests::restart_fixture(app);
    {
        let crate::app::root::Screen::Connected(project) = &mut restarted.screen else {
            unreachable!()
        };
        project.chat_slug = slug.clone();
        project.activity.manager_history = crate::persistence::manager_activity::load(&slug);
        project.planning_work = crate::core::planning_work::load(&project.state.planning_store)
            .expect("TaskIntent recovery action reloads from the private store");
    }
    restarted.advance_activity();
    let crate::app::root::Screen::Connected(project) = &restarted.screen else {
        unreachable!()
    };
    assert!(
        project
            .activity
            .manager_history_error
            .as_deref()
            .is_some_and(|error| { error.contains("sequence 3") })
    );
    let recovered_action = project
        .planning_work
        .iter()
        .find(|work| work.uid == action_uid)
        .expect("restart retains the same actionable TaskIntent blocker");
    assert_eq!(
        recovered_action.status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
    assert!(
        project
            .activity
            .pending
            .iter()
            .all(|event| { !event.contains("Investigate the saved TaskIntent blocker") })
    );
    assert!(
        crate::persistence::manager_activity::load(&slug)
            .iter()
            .any(
                |message| message.manager_run_event.as_ref().is_some_and(|event| {
                    event.sequence == 1 && event.checkpoint.as_ref() == Some(&checkpoint)
                })
            ),
        "restart retains the original checkpoint for diagnosis"
    );
}
