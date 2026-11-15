use super::*;

#[test]
fn liveness_recovery_is_dispatched_and_bounded_across_restarts() {
    let mut app = crate::app::root::board_tests::fixture();
    app.task_harness = Some(Box::new(ImmediateRecoveryFailure));
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let mut root = Work::new(
        "task:liveness-dispatch".into(),
        "Resume the approved goal".into(),
        "Continue the exact authorized operation.".into(),
        "Planning was interrupted".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    set_cycle(&mut root);
    let cycle_uid = root.task_intent_cycle_uid.clone().unwrap();
    let recovery = crate::core::planning_work::liveness_recovery_task(
        &root,
        &cycle_uid,
        "A durable recovery was queued before restart.",
    );
    project.planning_work = vec![root, recovery];
    project.active_turn = None;
    project.active_planning_work = None;
    project.activity.manager = None;
    project.task_turns.clear();

    let (key, request, feature_id) = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        next_recovery(project).expect("durable recovery is dispatchable")
    };
    app.start_turn_for_work(
        &request,
        crate::core::workflow::TurnPurpose::Interview,
        feature_id.as_deref(),
        Some(key),
    );
    let key = {
        let crate::app::root::Screen::Connected(project) = &app.screen else {
            unreachable!()
        };
        let key = project.active_planning_work.clone().unwrap();
        assert!(key.starts_with("task:manager-liveness:"));
        let recovery = project
            .planning_work
            .iter()
            .find(|work| work.key == key)
            .unwrap();
        assert_eq!(recovery.status, WorkStatus::InProgress);
        assert_eq!(recovery.liveness_recovery_attempts, 1);
        key
    };

    let mut restarted = crate::app::root::board_tests::restart_fixture(app);
    restarted.task_harness = Some(Box::new(ImmediateRecoveryFailure));
    let (retry_key, retry_request, retry_feature_id) = {
        let crate::app::root::Screen::Connected(project) = &mut restarted.screen else {
            unreachable!()
        };
        project.planning_work = crate::core::planning_work::load(&project.state.planning_store)
            .expect("reload persisted recovery task");
        reconcile(project);
        next_recovery(project).expect("interrupted recovery is queued for bounded retry")
    };
    assert_eq!(retry_key, key);
    restarted.start_turn_for_work(
        &retry_request,
        crate::core::workflow::TurnPurpose::Interview,
        retry_feature_id.as_deref(),
        Some(retry_key),
    );
    {
        let crate::app::root::Screen::Connected(project) = &restarted.screen else {
            unreachable!()
        };
        assert_eq!(project.active_planning_work.as_deref(), Some(key.as_str()));
        assert_eq!(
            project
                .planning_work
                .iter()
                .find(|work| work.key == key)
                .unwrap()
                .liveness_recovery_attempts,
            2
        );
    }

    let mut bounded = crate::app::root::board_tests::restart_fixture(restarted);
    if let crate::app::root::Screen::Connected(project) = &mut bounded.screen {
        project.planning_work = crate::core::planning_work::load(&project.state.planning_store)
            .expect("reload bounded recovery task");
        reconcile(project);
    }
    let crate::app::root::Screen::Connected(project) = &bounded.screen else {
        unreachable!()
    };
    let recovery = project
        .planning_work
        .iter()
        .find(|work| work.key == key)
        .unwrap();
    assert_eq!(recovery.status, WorkStatus::NeedsAttention);
    assert_eq!(recovery.liveness_recovery_attempts, 2);
    assert!(project.active_planning_work.is_none());
}
