use super::*;

/// Production boundary: ordinary app ticks reconcile and dispatch a persisted
/// TaskIntent recovery action. A reducer test cannot prove that repeated app
/// ticks reuse the one claimed action instead of queuing a duplicate. Local
/// private stores and a synthetic failing harness only; no external services;
/// setup cost is one headless app fixture.
#[test]
fn repeated_app_ticks_keep_one_recovery_action_and_claim() {
    let mut app = crate::app::root::board_tests::fixture();
    app.task_harness = Some(Box::new(ImmediateRecoveryFailure));
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let mut root = Work::new(
        "task:liveness-ticks".into(),
        "Continue the approved goal".into(),
        "Continue the exact authorized operation.".into(),
        "Planning was interrupted".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    set_cycle(&mut root);
    project.planning_work = vec![root];

    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        // Exercise real app ticks so any higher-priority planner work is
        // collected before the liveness recovery claims its normal turn slot.
        app.tick(0.016, &context);
        let crate::app::root::Screen::Connected(project) = &app.screen else {
            unreachable!()
        };
        let recoveries = project
            .planning_work
            .iter()
            .filter(|work| work.key.starts_with("task:manager-liveness:"))
            .collect::<Vec<_>>();
        assert_eq!(recoveries.len(), 1);
        if recoveries[0].status == WorkStatus::InProgress {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "repeated app ticks never claimed the single durable recovery action"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let crate::app::root::Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    let recoveries = project
        .planning_work
        .iter()
        .filter(|work| work.key.starts_with("task:manager-liveness:"))
        .collect::<Vec<_>>();
    assert_eq!(recoveries.len(), 1);
    assert_eq!(recoveries[0].status, WorkStatus::InProgress);
    assert_eq!(recoveries[0].liveness_recovery_attempts, 1);
    assert_eq!(
        project.active_planning_work.as_deref(),
        Some(recoveries[0].key.as_str())
    );
}

/// Production boundary: a finished scoped recovery turn is collected by the
/// ordinary app tick, reconciled into persisted planning work, and remains one
/// action across later ticks and a board-store reload. Direct dispatcher tests
/// cannot prove completion polling and result persistence. The harness is
/// synthetic; private files and a headless egui context only, no Docker or
/// network. Setup cost is one app fixture and one restart reload.
#[test]
fn completed_recovery_failure_converges_after_ticks_and_restart() {
    let mut app = crate::app::root::board_tests::fixture();
    app.task_harness = Some(Box::new(ImmediateRecoveryFailure));
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let mut root = Work::new(
        "task:liveness-completion".into(),
        "Continue the approved goal".into(),
        "Continue the exact authorized operation.".into(),
        "Planning was interrupted".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    set_cycle(&mut root);
    project.planning_work = vec![root];
    reconcile(project);
    let (key, request, feature_id) = next_recovery(project).expect("recovery task is eligible");
    assert!(key.starts_with("task:manager-liveness:"));
    app.start_turn_for_work(
        &request,
        crate::core::workflow::TurnPurpose::Interview,
        feature_id.as_deref(),
        Some(key),
    );

    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let recovery_key = loop {
        app.tick(0.0, &context);
        let crate::app::root::Screen::Connected(project) = &app.screen else {
            unreachable!()
        };
        if project.active_turn.is_none() && project.active_planning_work.is_none() {
            let recovery = project
                .planning_work
                .iter()
                .find(|work| work.key.starts_with("task:manager-liveness:"));
            if let Some(recovery) = recovery
                && recovery.status == WorkStatus::NeedsAttention
            {
                break recovery.key.clone();
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "completed recovery did not become a visible blocker"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    };

    app.advance_activity();
    app.advance_activity();
    let crate::app::root::Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    let matching = project
        .planning_work
        .iter()
        .filter(|work| work.key == recovery_key)
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].status, WorkStatus::NeedsAttention);
    assert!(
        matching[0].detail.contains("synthetic interruption"),
        "recovery blocker detail: {}",
        matching[0].detail
    );
    assert!(project.active_turn.is_none());

    let mut restarted = crate::app::root::board_tests::restart_fixture(app);
    let crate::app::root::Screen::Connected(project) = &mut restarted.screen else {
        unreachable!()
    };
    project.planning_work = crate::core::planning_work::load(&project.state.planning_store)
        .expect("reload completed recovery result");
    restarted.task_harness = Some(Box::new(ImmediateRecoveryFailure));
    restarted.advance_activity();
    restarted.advance_activity();
    let crate::app::root::Screen::Connected(project) = &restarted.screen else {
        unreachable!()
    };
    let persisted_recoveries = project
        .planning_work
        .iter()
        .filter(|work| work.key == recovery_key)
        .collect::<Vec<_>>();
    assert_eq!(persisted_recoveries.len(), 1);
    assert_eq!(persisted_recoveries[0].status, WorkStatus::NeedsAttention);
    assert_eq!(persisted_recoveries[0].liveness_recovery_attempts, 1);
    assert!(
        persisted_recoveries[0]
            .detail
            .contains("synthetic interruption")
    );
    assert!(project.active_turn.is_none());
}
