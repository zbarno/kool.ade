use super::*;

#[test]
fn unready_plan_remains_visible_but_cannot_be_confirmed() {
    // Production boundary: the board projects a pending plan even when readiness is blocked.
    // Invariant: users can find the card, but readiness still blocks confirmation.
    // External dependencies: none. Setup cost: one temporary Git repository.
    let _shield = crate::core::gitops::test_support::shield("plan-confirmation-readiness-card");
    let (mut app, root, _) = setup();
    let snapshot = current_snapshot(&app, "CHG-004");
    let Screen::Connected(project) = &mut app.screen else {
        panic!("project remains connected")
    };
    let questions = crate::core::planning::QuestionStore::new(&project.state.planning_store);
    let (mut readiness, revision) = questions
        .load_readiness(&snapshot.feature_uid)
        .unwrap()
        .unwrap();
    readiness.ready = false;
    readiness.revision += 1;
    questions.save_readiness(&readiness, revision).unwrap();
    project.state.baseline_planning_revision = project.state.planning_store.revision().unwrap();
    let actions = app.feature_actions(None);
    assert_eq!(actions.len(), 1, "blocked plan must not disappear from the board");
    assert_eq!(actions[0].id, "CHG-004");
    assert!(!actions[0].ready);
    assert!(!actions[0].approved);
    app.approve_feature_from_board("CHG-004");
    assert!(!app.feature_approved("CHG-004"), "readiness gate must still fail closed");
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
