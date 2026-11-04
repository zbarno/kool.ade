use super::*;

#[test]
fn plan_choice_is_persisted_before_feature_approval_is_allowed() {
    let _shield = crate::core::gitops::test_support::shield("feature-plan-choice");
    let (mut app, root, _) = setup();
    let store = match &app.screen {
        Screen::Connected(project) => project.state.planning_store.clone(),
        Screen::Welcome => panic!("fixture remains connected"),
    };
    let store_root = store.root.clone();
    let mut markdown = crate::artifacts::product_docs::current_document(&store, "feature:CHG-004")
        .unwrap()
        .unwrap();
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
    let changes = crate::core::apply::private_document_draft_changes(
        &store,
        &[("feature:CHG-004".into(), markdown)],
    )
    .unwrap();
    store.transaction(&changes, None).unwrap();
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

    let comparison: crate::domain::PlanComparison = serde_json::from_value(serde_json::json!({
        "alternatives": [
            {"id":"A","objective":"Low risk","phases":[{"name":"Prepare","subtasks":["Add shadow storage"]},{"name":"Switch","subtasks":["Route reads"]},{"name":"Verify","subtasks":["Check results"]}],"filesTouched":["src/a.rs"],"stateChanges":["Add shadow storage"],"failureModes":["Switch fails"],"effortBand":"Small — one module","knownRisks":["Temporary duplication"],"reversibility":"Remove shadow storage"},
            {"id":"B","objective":"Faster cutover","phases":[{"name":"Build","subtasks":["Replace storage"]},{"name":"Switch","subtasks":["Move reads"]},{"name":"Verify","subtasks":["Check results"]}],"filesTouched":["src/b.rs"],"stateChanges":["Replace storage"],"failureModes":["Migration fails"],"effortBand":"Medium — migration","knownRisks":["Cutover risk"],"reversibility":"Restore backup"}
        ],
        "recommendation":{"plan_id":"A","rationale":"Lower transition risk","evidence":["src/a.rs"]},
        "selected_plan":null
    })).unwrap();
    if let Screen::Connected(project) = &mut app.screen {
        project
            .state
            .workflow
            .approved_features
            .insert("CHG-004".into(), "stale contract".into());
        crate::artifacts::task_docs::save_workflow(
            &project.state.planning_store,
            &project.state.workflow,
        )
        .unwrap();
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    if let Screen::Connected(project) = &mut app.screen {
        let mut workflow =
            crate::artifacts::task_docs::load_workflow(&project.state.planning_store).unwrap();
        workflow.plan_comparisons.insert(
            "CHG-004".into(),
            crate::core::workflow::PlanComparisonRecord {
                schema_version: 1,
                feature_id: "CHG-004".into(),
                alternatives: comparison,
                history: Vec::new(),
                transcript: "Synthetic comparison for the plan-choice test.".into(),
                status: crate::core::workflow::PlanComparisonStatus::Proposed,
                selected_plan: None,
                updated_at_ms: 1,
            },
        );
        crate::artifacts::task_docs::save_workflow(&project.state.planning_store, &workflow)
            .unwrap();
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    assert_eq!(
        app.feature_actions(None)[0].label(),
        "Choose a plan for CHG-004 below"
    );
    let mut saved_workflow = crate::artifacts::task_docs::load_workflow(&store_root).unwrap();
    assert!(
        crate::core::workflow::approve_feature_if_current(
            &store_root,
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
    let saved = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == "CHG-004")
        .unwrap()
        .1
        .as_str();
    super::compare::assert_feature_metadata_has_no_plan_state(saved);
    let persisted = crate::artifacts::task_docs::load_workflow(&store_root).unwrap();
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
    let decisions = store_root.join("planning/decisions");
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
        "Confirm plan and allow publication for CHG-004"
    );
    record_test_plan_author(&mut app, "CHG-004");
    app.approve_feature_only("CHG-004");
    record_test_plan_review_and_merge(
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
