use super::*;

#[test]
fn plan_choice_is_persisted_before_feature_approval_is_allowed() {
    let _shield = crate::core::gitops::test_support::shield("feature-plan-choice");
    let (mut app, root, _) = setup();
    let path =
        root.join(".koolade-packet/planning/changes/CHG-004-saved-searches/specification.md");
    let mut markdown = std::fs::read_to_string(&path).unwrap();
    let marker = markdown
        .lines()
        .find(|line| line.starts_with("<!-- koolade-change:v1 "))
        .unwrap()
        .to_owned();
    let json = marker
        .strip_prefix("<!-- koolade-change:v1 ")
        .unwrap()
        .strip_suffix(" -->")
        .unwrap();
    let mut metadata: serde_json::Value = serde_json::from_str(json).unwrap();
    metadata["schemaVersion"] = 2.into();
    markdown = markdown.replace(
        &marker,
        &format!(
            "<!-- koolade-change:v1 {} -->",
            serde_json::to_string(&metadata).unwrap()
        ),
    );
    std::fs::write(&path, &markdown).unwrap();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        let body = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == "CHG-004")
            .unwrap()
            .1
            .clone();
        readiness::seed_test_readiness(&project.state.planning_store, "CHG-004", &body);
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    assert_eq!(
        app.feature_actions(None)[0].label(),
        "Compare plans for CHG-004"
    );

    let marker = markdown
        .lines()
        .find(|line| line.starts_with("<!-- koolade-change:v1 "))
        .unwrap()
        .to_owned();
    let json = marker
        .strip_prefix("<!-- koolade-change:v1 ")
        .unwrap()
        .strip_suffix(" -->")
        .unwrap();
    metadata = serde_json::from_str(json).unwrap();
    metadata["planComparison"] = serde_json::json!({
        "alternatives": [
            {"id":"A","objective":"Low risk","phases":[{"name":"Prepare","subtasks":["Add shadow storage"]},{"name":"Switch","subtasks":["Route reads"]},{"name":"Verify","subtasks":["Check results"]}],"filesTouched":["src/a.rs"],"stateChanges":["Add shadow storage"],"failureModes":["Switch fails"],"effortBand":"Small — one module","knownRisks":["Temporary duplication"],"reversibility":"Remove shadow storage"},
            {"id":"B","objective":"Faster cutover","phases":[{"name":"Build","subtasks":["Replace storage"]},{"name":"Switch","subtasks":["Move reads"]},{"name":"Verify","subtasks":["Check results"]}],"filesTouched":["src/b.rs"],"stateChanges":["Replace storage"],"failureModes":["Migration fails"],"effortBand":"Medium — migration","knownRisks":["Cutover risk"],"reversibility":"Restore backup"}
        ],
        "recommendation":{"plan_id":"A","rationale":"Lower transition risk","evidence":["src/a.rs"]},
        "selected_plan":null
    });
    markdown = markdown.replace(
        &marker,
        &format!(
            "<!-- koolade-change:v1 {} -->",
            serde_json::to_string(&metadata).unwrap()
        ),
    );
    std::fs::write(&path, markdown).unwrap();
    if let Screen::Connected(project) = &mut app.screen {
        project
            .state
            .workflow
            .approved_features
            .insert("CHG-004".into(), "stale contract".into());
        crate::artifacts::task_docs::save_workflow(&root, &project.state.workflow).unwrap();
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    assert_eq!(
        app.feature_actions(None)[0].label(),
        "Choose a plan for CHG-004 below"
    );
    let mut saved_workflow = crate::artifacts::task_docs::load_workflow(&root).unwrap();
    assert!(
        crate::core::workflow::approve_feature_if_current(
            &root,
            &mut saved_workflow,
            "CHG-004",
            None,
        )
        .is_err()
    );
    assert!(!app.feature_approved("CHG-004"));
    app.approve_feature_only("CHG-004");
    assert!(!app.feature_approved("CHG-004"));
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        let body = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == "CHG-004")
            .unwrap()
            .1
            .clone();
        readiness::seed_test_readiness(&project.state.planning_store, "CHG-004", &body);
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    assert_eq!(
        app.feature_actions(None)[0].label(),
        "Choose a plan for CHG-004 below"
    );

    app.dispatch(crate::ui::ApplicationCommand::ChooseFeaturePlan {
        id: "CHG-004".into(),
        plan_id: "B".into(),
    });
    let Screen::Connected(project) = &app.screen else {
        panic!()
    };
    let saved = crate::domain::ChangeMetadata::require_markdown(
        &project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == "CHG-004")
            .unwrap()
            .1,
    )
    .unwrap();
    assert!(saved.plan_comparison.is_none());
    assert!(saved.selected_alt.is_none());
    let persisted = crate::artifacts::task_docs::load_workflow(&root).unwrap();
    let record = persisted.plan_comparisons.get("CHG-004").unwrap();
    assert_eq!(
        record.status,
        crate::core::workflow::PlanComparisonStatus::Adopted
    );
    assert_eq!(record.selected_plan.as_deref(), Some("B"));
    record.validate().unwrap();
    assert!(
        !project
            .state
            .workflow
            .approved_features
            .contains_key("CHG-004")
    );
    assert!(
        project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == "CHG-004")
            .unwrap()
            .1
            .contains("## Selected Plan")
    );
    let decisions = root.join(".koolade-packet/planning/decisions");
    let adr = std::fs::read_dir(decisions)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let adr = std::fs::read_to_string(adr).unwrap();
    assert!(adr.contains("Option A"));
    assert!(adr.contains("Variation axes"));
    assert!(
        crate::core::workflow::feature_contract(&project.state.active_feature.as_ref().unwrap().1)
            .contains("Selected Plan")
    );
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        let body = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == "CHG-004")
            .unwrap()
            .1
            .clone();
        readiness::seed_test_readiness(&project.state.planning_store, "CHG-004", &body);
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    assert_eq!(
        app.feature_actions(None)[0].label(),
        "Confirm plan for CHG-004"
    );
    record_test_plan_author(&mut app, "CHG-004");
    app.approve_feature_only("CHG-004");
    record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    assert!(
        app.feature_approved("CHG-004"),
        "{:?}",
        app.chat_messages().last()
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
