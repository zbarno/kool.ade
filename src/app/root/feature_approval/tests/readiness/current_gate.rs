use super::super::*;
use super::current_snapshot;

/// Covers the app task-generation boundary: an unchanged snapshot cannot
/// generate tasks after its persisted readiness assessment becomes blocking.
/// A lifecycle unit test cannot prove both UI action availability and direct
/// dispatch enforce this boundary. Uses only a temporary local planning store;
/// no external services, with the normal app fixture setup cost.
#[test]
fn changed_readiness_blocks_existing_confirmation_and_review() {
    let _shield = crate::core::gitops::test_support::shield("approval-readiness-current");
    let (mut app, root, _) = setup();
    record_test_plan_author(&mut app, "CHG-004");
    app.approve_feature_only("CHG-004");
    record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    app.approve_feature_from_board("CHG-004");

    let (snapshot, work_key) = {
        let Screen::Connected(project) = &app.screen else {
            panic!("project remains connected");
        };
        let snapshot = current_snapshot(&app, "CHG-004");
        let key = project
            .planning_work
            .iter()
            .find(|work| work.kind == crate::core::planning_work::WorkKind::TaskGeneration)
            .unwrap()
            .key
            .clone();
        (snapshot, key)
    };
    {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("project remains connected");
        };
        let questions = crate::core::planning::QuestionStore::new(&project.state.planning_store);
        let (mut readiness, revision) = questions
            .load_readiness(&snapshot.feature_uid)
            .unwrap()
            .unwrap();
        readiness.ready = false;
        readiness.unresolved_blocker_ids.clear();
        readiness.blocker_notes = vec!["A current blocker was found.".into()];
        readiness.revision += 1;
        questions.save_readiness(&readiness, revision).unwrap();
    }

    let actions = app.feature_actions(None);
    assert!(
        actions
            .iter()
            .any(|action| action.id == "CHG-004" && !action.prepare_tasks)
    );
    app.start_task_generation(&work_key, "CHG-004");
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected");
    };
    let work = project
        .planning_work
        .iter()
        .find(|work| work.key == work_key)
        .unwrap();
    assert_eq!(
        work.status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
    assert!(
        work.detail
            .contains("readiness evidence is missing or stale")
    );
    assert!(project.active_turn.is_none());
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

/// Covers both the Board action and direct task-generation dispatch: an approved
/// receipt cannot bypass an unresolved Manager contest. A lifecycle-only test
/// cannot prove this app boundary. Uses the temporary local planning store and
/// standard app fixture; no external services.
#[test]
fn pending_manager_contest_blocks_task_generation_actions_and_dispatch() {
    let _shield = crate::core::gitops::test_support::shield("approval-contest-current");
    let (mut app, root, _) = setup();
    record_test_plan_author(&mut app, "CHG-004");
    app.approve_feature_only("CHG-004");
    record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    app.approve_feature_from_board("CHG-004");

    let (work_key, snapshot_hash) = {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("project remains connected");
        };
        let generation = project
            .planning_work
            .iter()
            .find(|work| work.kind == crate::core::planning_work::WorkKind::TaskGeneration)
            .unwrap();
        let key = generation.key.clone();
        let markdown = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == "CHG-004")
            .unwrap()
            .1
            .clone();
        let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
            .unwrap()
            .unwrap();
        let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
            &project.state.planning_store,
            "CHG-004",
            &identity,
            &markdown,
            &crate::core::workflow::feature_contract(&markdown),
        )
        .unwrap();
        (key, snapshot.content_hash)
    };
    {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("project remains connected");
        };
        let mut contest = crate::core::planning_work::Work::new(
            String::new(),
            "Independent arbitration: F-1".into(),
            "Review the contested finding".into(),
            format!("Snapshot hash: {snapshot_hash}"),
        );
        contest.kind = crate::core::planning_work::WorkKind::PlanReviewDisposition;
        contest.status = crate::core::planning_work::WorkStatus::NeedsAttention;
        contest.feature_id = Some("CHG-004".into());
        contest.key = format!("task:manager-plan-contest:{snapshot_hash}:F-1");
        project.planning_work.push(contest);
        project.save_planning_work().unwrap();
    }

    assert!(
        !app.feature_actions(None)
            .iter()
            .any(|action| action.id == "CHG-004" && action.prepare_tasks)
    );
    app.start_task_generation(&work_key, "CHG-004");
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected");
    };
    let generation = project
        .planning_work
        .iter()
        .find(|work| work.key == work_key)
        .unwrap();
    assert_eq!(
        generation.status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
    assert!(generation.detail.contains("Manager-contested plan finding"));
    assert!(project.active_turn.is_none());
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
