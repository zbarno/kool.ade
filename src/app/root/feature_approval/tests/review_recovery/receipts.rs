use super::*;

#[test]
fn confirmed_plan_can_resume_review_without_replacing_user_receipt() {
    // Production boundary: persisted PlanningStore confirmation is reloaded into a new Project and the board action resumes the missing review.
    // Invariant: restart recovery never writes a replacement user receipt; a unit test of the retry predicate cannot prove Project reload wiring.
    // External dependencies: none. Setup cost: one temporary Git repository and PlanningStore fixture.
    let _shield = crate::core::gitops::test_support::shield("plan-review-restart-recovery");
    let (mut app, root, _) = setup();
    app.approve_feature_from_board("CHG-004");
    let (snapshot, confirmation_before) = match &app.screen {
        Screen::Connected(project) => {
            let (_, markdown) = project
                .state
                .active_features
                .iter()
                .find(|(id, _)| id == "CHG-004")
                .unwrap();
            let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
                .unwrap()
                .unwrap();
            let contract = workflow::feature_contract(markdown);
            let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
                &project.state.planning_store,
                "CHG-004",
                &identity,
                markdown,
                &contract,
            )
            .unwrap();
            let receipt = crate::core::lifecycle::plan_snapshot::user_confirmation_receipt(
                &project.state.planning_store,
                &snapshot,
            )
            .unwrap()
            .unwrap();
            (snapshot, receipt)
        }
        Screen::Welcome => panic!("project remains connected"),
    };
    app.screen = Screen::Connected(Box::new(crate::app::session::test_support::project_from(
        &root,
    )));
    assert_eq!(
        app.feature_actions(None)[0].label(),
        "Start or retry independent plan review for CHG-004"
    );

    app.approve_feature_from_board("CHG-004");

    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    assert_eq!(
        crate::core::lifecycle::plan_snapshot::user_confirmation_receipt(
            &project.state.planning_store,
            &snapshot,
        )
        .unwrap(),
        Some(confirmation_before),
        "resuming review must not replace the explicit user approval"
    );
    assert_eq!(snapshot.schema_version, 3);
    assert_eq!(
        snapshot.original_goal,
        project.state.workflow.brief.as_ref().unwrap().goal
    );
    assert!(
        crate::core::lifecycle::plan_snapshot::plan_review_receipt(
            &project.state.planning_store,
            &snapshot,
        )
        .unwrap()
        .is_none(),
        "a test-only unavailable reviewer must not fabricate a review receipt"
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn changing_the_persisted_original_goal_invalidates_confirmation() {
    let _shield = crate::core::gitops::test_support::shield("plan-review-goal-binding");
    let (mut app, root, _) = setup();
    app.approve_feature_from_board("CHG-004");
    let before = match &app.screen {
        Screen::Connected(project) => {
            let (_, markdown) = project
                .state
                .active_features
                .iter()
                .find(|(id, _)| id == "CHG-004")
                .unwrap();
            let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
                .unwrap()
                .unwrap();
            crate::core::lifecycle::plan_snapshot::from_feature_with_author(
                &project.state.planning_store,
                "CHG-004",
                &identity,
                markdown,
                &workflow::feature_contract(markdown),
            )
            .unwrap()
        }
        Screen::Welcome => panic!("project remains connected"),
    };
    let Screen::Connected(project) = &mut app.screen else {
        panic!("project remains connected")
    };
    project.state.workflow.brief.as_mut().unwrap().goal =
        "A materially different user goal must not reuse the old approval.".into();
    crate::artifacts::task_docs::save_workflow(&root, &project.state.workflow).unwrap();
    project.state = crate::core::state::PlannerState::load(&root).unwrap();
    let (_, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == "CHG-004")
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    let after = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        "CHG-004",
        &identity,
        markdown,
        &workflow::feature_contract(markdown),
    )
    .unwrap();
    assert_ne!(before.content_hash, after.content_hash);
    assert_ne!(before.spec_revision, after.spec_revision);
    assert!(
        !crate::core::lifecycle::plan_snapshot::has_current_user_confirmation(
            &project.state.planning_store,
            &after,
        )
        .unwrap()
    );
    let markdown = markdown.clone();
    readiness::seed_test_readiness(&project.state.planning_store, "CHG-004", &markdown);
    project.state = crate::core::state::PlannerState::load(&root).unwrap();
    let candidate_keys = app.feature_actions(None);
    assert_eq!(candidate_keys.len(), 1);
    assert_eq!(
        candidate_keys[0].board_key(),
        format!(
            "plan-confirmation:{}:{}",
            after.feature_uid, after.content_hash
        ),
        "the board confirmation identity binds the active feature and exact candidate hash"
    );
    assert_ne!(
        format!(
            "plan-confirmation:{}:{}",
            before.feature_uid, before.content_hash
        ),
        candidate_keys[0].board_key(),
        "a changed candidate must receive a new confirmation card identity"
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
