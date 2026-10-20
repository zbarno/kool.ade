use super::*;

#[test]
fn approved_fresh_review_closes_only_older_manager_repair_work() {
    // Production boundary: persisted Manager repair cards are reconciled against an approved reviewer receipt for the current snapshot.
    // Invariant: only work for the reviewed feature and an older candidate closes; snapshot and receipt provenance remain on the card.
    // External dependencies: none. Setup cost: one temporary Git repository and planning-work fixture.
    let _shield = crate::core::gitops::test_support::shield("manager-plan-repair-verification");
    let (mut app, root, _) = setup();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("project remains connected")
    };
    for (key, snapshot_hash, status) in [
        (
            "task:manager-plan-repair:old:finding-1",
            "old-hash",
            crate::core::planning_work::WorkStatus::InReview,
        ),
        (
            "task:manager-plan-repair:todo:finding-3",
            "older-hash",
            crate::core::planning_work::WorkStatus::Todo,
        ),
        (
            "task:manager-plan-repair:current:finding-2",
            "current-hash",
            crate::core::planning_work::WorkStatus::InReview,
        ),
    ] {
        let mut work = crate::core::planning_work::Work::new(
            String::new(),
            "Manager repair".into(),
            "Repair the reviewed finding".into(),
            format!("Manager disposition: RepairPlan\nSnapshot hash: {snapshot_hash}"),
        );
        work.key = key.into();
        work.kind = crate::core::planning_work::WorkKind::PlanReviewDisposition;
        work.feature_id = Some("CHG-004".into());
        work.status = status;
        project.planning_work.push(work);
    }
    project.save_planning_work().unwrap();

    assert!(
        crate::app::root::tick::activity::plan_review::lifecycle::verify_manager_plan_repairs(
            project,
            "CHG-004",
            "current-hash",
            "review-receipt-1",
        )
        .unwrap()
    );
    let repaired = project
        .planning_work
        .iter()
        .find(|work| work.key == "task:manager-plan-repair:old:finding-1")
        .unwrap();
    assert_eq!(
        repaired.status,
        crate::core::planning_work::WorkStatus::Done
    );
    assert!(repaired.detail.contains("review-receipt-1"));
    let not_started = project
        .planning_work
        .iter()
        .find(|work| work.key == "task:manager-plan-repair:todo:finding-3")
        .unwrap();
    assert_eq!(
        not_started.status,
        crate::core::planning_work::WorkStatus::Done
    );
    let same_candidate = project
        .planning_work
        .iter()
        .find(|work| work.key == "task:manager-plan-repair:current:finding-2")
        .unwrap();
    assert_eq!(
        same_candidate.status,
        crate::core::planning_work::WorkStatus::InReview
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unresolved_manager_repair_hides_and_blocks_task_generation() {
    // Production boundary: approved plan, pending Manager repair, Board action projection, and direct task-generation dispatch are exercised together.
    // Invariant: an unresolved repair prevents task generation even if a user calls the generation action directly; a pure predicate test cannot prove both application routes enforce the gate.
    // External dependencies: none. Setup cost: one temporary Git repository and persisted approval/review fixtures.
    let _shield = crate::core::gitops::test_support::shield("manager-plan-repair-task-gate");
    let (mut app, root, _) = setup();
    record_test_plan_author(&mut app, "CHG-004");
    app.approve_feature_only("CHG-004");
    record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    app.approve_feature_from_board("CHG-004");
    let task_key = match &mut app.screen {
        Screen::Connected(project) => {
            let task_key = project
                .planning_work
                .iter()
                .find(|work| work.kind == crate::core::planning_work::WorkKind::TaskGeneration)
                .unwrap()
                .key
                .clone();
            let mut repair = crate::core::planning_work::Work::new(
                String::new(),
                "Manager repair".into(),
                "Resolve the reviewed finding".into(),
                "Snapshot hash: prior-candidate".into(),
            );
            repair.key = "task:manager-plan-repair:prior-candidate:F-1".into();
            repair.kind = crate::core::planning_work::WorkKind::PlanReviewDisposition;
            repair.feature_id = Some("CHG-004".into());
            repair.status = crate::core::planning_work::WorkStatus::InReview;
            project.planning_work.push(repair);
            project.save_planning_work().unwrap();
            task_key
        }
        Screen::Welcome => panic!("project remains connected"),
    };
    assert!(app.feature_approved("CHG-004"));
    assert!(
        app.available_feature_actions(None)
            .iter()
            .all(|action| !action.prepare_tasks),
        "unverified Manager repair must hide the task-generation action"
    );

    app.start_task_generation(&task_key, "CHG-004");
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    assert!(project.active_turn.is_none());
    let task_generation = project
        .planning_work
        .iter()
        .find(|work| work.key == task_key)
        .unwrap();
    assert_eq!(
        task_generation.status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
    assert!(
        task_generation
            .detail
            .contains("Manager-directed plan repair")
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
