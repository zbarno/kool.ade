use super::*;

/// Production boundary: persisted Manager JSONL is loaded by ordinary app
/// activity, validated against its event sequence, and blocked from replay when
/// the sequence has a gap. In the same restart path an existing TaskIntent
/// recovery action remains singular and actionable. Reducer tests cannot prove
/// Manager failure and lifecycle progress converge together. Uses synthetic
/// local stores only; no Docker, SSH, provider, or network; moderate app setup.
#[test]
fn restart_blocks_manager_gap_while_preserving_taskintent_action() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = std::env::temp_dir().join(format!("manager-gap-home-{}", std::process::id()));
    environment.set("KOOLADE_HOME", &home);
    let slug = format!("manager-gap-{}", uuid::Uuid::new_v4());
    let checkpoint = crate::domain::ManagerEventCheckpointV1::from_messages(
        &["Review the durable TaskIntent state".into()],
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
        checkpoint: (sequence == 1 || outcome == crate::domain::ManagerRunOutcomeV1::Started)
            .then(|| checkpoint.clone()),
        checkpoint_cursor: checkpoint.cursor,
        checkpoint_sha256: checkpoint.sha256.clone(),
        retry_after_unix_ms: None,
    };
    let mut first = ChatMessage::new(ChatRole::System, "Manager run queued", None);
    first.manager_run_event = Some(event(1, crate::domain::ManagerRunOutcomeV1::Queued));
    let mut gap = ChatMessage::new(ChatRole::System, "Manager start checkpoint", None);
    gap.manager_run_event = Some(event(3, crate::domain::ManagerRunOutcomeV1::Started));
    crate::persistence::manager_activity::append(&slug, &[first, gap]).unwrap();

    let mut app = crate::app::root::board_tests::fixture();
    {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        project.chat_slug = slug.clone();
        project.activity.manager_history = crate::persistence::manager_activity::load(&slug);
        let mut root = crate::core::planning_work::Work::new(
            "task:manager-gap-root".into(),
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
            "task:manager-gap-action".into(),
            "Review the interrupted Manager event".into(),
            "Resolve the preserved supervision checkpoint.".into(),
            "Manager event sequence is incomplete.".into(),
        );
        action.status = crate::core::planning_work::WorkStatus::NeedsAttention;
        action.parent_uid = Some(root.uid.clone());
        action.task_intent_uid = Some(root.uid.clone());
        action.task_intent_cycle_uid = Some(cycle_uid);
        project.planning_work = vec![root, action];
        project.save_planning_work().unwrap();
    }
    app.advance_activity();
    let crate::app::root::Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    let error = project.activity.manager_history_error.clone().unwrap();
    assert!(error.contains("sequence 3"));
    assert!(
        project
            .activity
            .pending
            .iter()
            .all(|message| !message.contains("Review the durable TaskIntent state")),
        "gap events are not replayed"
    );
    assert_eq!(
        project
            .activity
            .manager_history
            .iter()
            .filter(|message| message.text == error)
            .count(),
        1
    );
    assert_eq!(project.planning_work.len(), 2);
    let action_uid = project
        .planning_work
        .iter()
        .find(|work| work.key == "task:manager-gap-action")
        .unwrap()
        .uid
        .clone();
    assert_eq!(
        project
            .planning_work
            .iter()
            .find(|work| work.uid == action_uid)
            .unwrap()
            .status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );

    let mut restarted = crate::app::root::board_tests::restart_fixture(app);
    {
        let crate::app::root::Screen::Connected(project) = &mut restarted.screen else {
            unreachable!()
        };
        project.chat_slug = slug.clone();
        project.activity.manager_history = crate::persistence::manager_activity::load(&slug);
        project.planning_work = crate::core::planning_work::load(&project.state.planning_store)
            .expect("TaskIntent action reloads with the private PlanningStore");
    }
    restarted.advance_activity();
    restarted.advance_activity();
    let crate::app::root::Screen::Connected(project) = &restarted.screen else {
        unreachable!()
    };
    assert_eq!(
        project.activity.manager_history_error.as_deref(),
        Some(error.as_str())
    );
    assert!(
        project
            .activity
            .pending
            .iter()
            .all(|message| { !message.contains("Review the durable TaskIntent state") })
    );
    assert_eq!(
        project
            .activity
            .manager_history
            .iter()
            .filter(|message| message.text == error)
            .count(),
        1
    );
    assert_eq!(project.planning_work.len(), 2);
    let action = project
        .planning_work
        .iter()
        .find(|work| work.key == "task:manager-gap-action")
        .unwrap();
    assert_eq!(action.uid, action_uid);
    assert_eq!(
        action.status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
}
