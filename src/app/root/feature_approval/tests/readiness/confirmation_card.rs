use super::*;

#[test]
fn plan_confirmation_card_is_not_created_until_readiness_passes() {
    // Production boundary: the board action projection consumes persisted readiness before exposing a confirmation card.
    // Invariant: a feature with failed readiness has no confirm/request action; testing only the approval command would miss a misleading card.
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
    assert!(
        actions.is_empty(),
        "a failed readiness assessment exposed {} feature action(s)",
        actions.len()
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
