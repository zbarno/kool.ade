use super::*;

#[test]
fn changed_contract_stops_authorized_continuation_and_manager_uses_current_state() {
    let _shield = crate::core::gitops::test_support::shield("feature-approval-continuation");
    let (mut app, root, review) = setup();
    let (h, _) = harness(vec![review.to_string()]);
    app.task_harness = Some(h);
    app.approve_and_prepare_feature("CHG-004");
    assert!(finish(&mut app));
    assert!(
        !app.chat_messages()
            .last()
            .unwrap()
            .text
            .contains("Would you like to proceed")
    );
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    let prompt = crate::app::manager::Manager::prompt_body(p, &[]);
    assert!(prompt.contains("CHG-004: approved for current contract; do not ask again"));
    assert!(prompt.contains("Persist saved searches and restore them after restarting."));
    let path =
        root.join(".koolade-packet/planning/changes/CHG-004-saved-searches/specification.md");
    let changed = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Persist saved searches", "Publish saved searches");
    std::fs::write(&path, changed).unwrap();
    p.state = crate::core::state::PlannerState::load(&root).unwrap();
    app.continue_feature_generation(true);
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    assert!(p.active_turn.is_none());
    assert!(app.pending_feature_generation.is_none());
    assert!(
        app.chat_messages()
            .last()
            .unwrap()
            .text
            .contains("contract or planning focus changed")
    );
    assert_eq!(
        app.feature_actions(None)[0].label(),
        "Approve CHG-004 and prepare tasks"
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_display_cannot_approve_changed_contract_or_overwrite_other_approval() {
    let _shield = crate::core::gitops::test_support::shield("feature-approval-drift");
    let (mut app, root, _) = setup();
    let path =
        root.join(".koolade-packet/planning/changes/CHG-004-saved-searches/specification.md");
    let original = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        original.replace("Persist saved searches", "Publish saved searches"),
    )
    .unwrap();
    app.approve_and_prepare_feature("CHG-004");
    assert!(!app.feature_approved("CHG-004"));
    assert!(
        app.chat_messages()
            .last()
            .unwrap()
            .text
            .contains("changed since it was displayed")
    );
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    assert!(p.active_turn.is_none());
    std::fs::write(&path, original).unwrap();
    let mut saved = crate::artifacts::task_docs::load_workflow(&root).unwrap();
    saved
        .approved_features
        .insert("CHG-009".into(), "Other contract".into());
    crate::artifacts::task_docs::save_workflow(&root, &saved).unwrap();
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    workflow::approve_feature_if_current(
        &root,
        &mut p.state.workflow,
        "CHG-004",
        Some(&workflow::feature_contract(
            &p.state.active_feature.as_ref().unwrap().1,
        )),
    )
    .unwrap();
    assert_eq!(
        p.state.workflow.approved_features["CHG-009"],
        "Other contract"
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
