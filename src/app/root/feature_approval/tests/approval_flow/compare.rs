use super::*;

pub(super) fn assert_feature_metadata_has_no_plan_state(markdown: &str) {
    let marker = markdown
        .lines()
        .find(|line| line.starts_with("<!-- koolade-change:v1 "))
        .expect("feature metadata marker is present");
    let json = marker
        .strip_prefix("<!-- koolade-change:v1 ")
        .unwrap()
        .strip_suffix(" -->")
        .unwrap();
    let metadata: serde_json::Value = serde_json::from_str(json).unwrap();
    for field in [
        "planComparison",
        "comparisonHistory",
        "selectedAlt",
        "selectedPlan",
        "adoption",
    ] {
        assert!(
            metadata.get(field).is_none(),
            "feature metadata retains workflow plan state field {field}: {metadata}"
        );
    }
}

#[test]
fn compare_plans_turn_persists_two_options_for_the_ready_feature() {
    let _shield = crate::core::gitops::test_support::shield("feature-plan-comparison-turn");
    let (mut app, root, _) = setup();
    let store = match &app.screen {
        Screen::Connected(project) => project.state.planning_store.clone(),
        Screen::Welcome => panic!("fixture remains connected"),
    };
    let body = crate::artifacts::product_docs::current_document(&store, "feature:CHG-004")
        .unwrap()
        .unwrap();
    let marker = body
        .lines()
        .find(|line| line.starts_with("<!-- koolade-change:v1 "))
        .unwrap();
    let json = marker
        .strip_prefix("<!-- koolade-change:v1 ")
        .unwrap()
        .strip_suffix(" -->")
        .unwrap();
    let mut metadata: serde_json::Value = serde_json::from_str(json).unwrap();
    metadata["schemaVersion"] = 2.into();
    let body = body.replace(
        marker,
        &format!(
            "<!-- koolade-change:v1 {} -->",
            serde_json::to_string(&metadata).unwrap()
        ),
    );
    let changes = crate::core::apply::private_document_draft_changes(
        &store,
        &[("feature:CHG-004".into(), body)],
    )
    .unwrap();
    store.transaction(&changes, None).unwrap();
    if let Screen::Connected(project) = &mut app.screen {
        let store = project.state.planning_store.clone();
        project.state = crate::core::state::PlannerState::load_with_store(&root, &store).unwrap();
        let body = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == "CHG-004")
            .unwrap()
            .1
            .clone();
        readiness::seed_test_readiness(&project.state.planning_store, "CHG-004", &body);
        let store = project.state.planning_store.clone();
        project.state = crate::core::state::PlannerState::load_with_store(&root, &store).unwrap();
    }
    let option = |id: &str, path: &str, phase: &str| {
        serde_json::json!({
            "id": id, "objective": format!("Use {phase} for saved searches"),
            "phases": [
                {"name": phase, "subtasks": [format!("Implement {phase} persistence")]},
                {"name": "Restore", "subtasks": ["Load a complete record"]},
                {"name": "Verify", "subtasks": ["Check restart behavior"]}
            ], "files_touched": [path], "state_changes": [format!("Store {phase} records")],
            "failure_modes": [format!("{phase} write fails")], "effort_band": "Small — one module",
            "known_risks": [format!("{phase} compatibility")], "reversibility": format!("Remove {phase} storage")
        })
    };
    let response = serde_json::json!({
        "schema_version": 2, "assistant_message": "Here are two approaches.",
        "plans": [option("A", "src/search_store.rs", "Atomic"), option("B", "src/search_migration.rs", "Staged")],
        "recommendation": {"plan_id": "A", "rationale": "Fewer transition steps.", "evidence": ["CHG-004 acceptance criteria"]}
    });
    app.task_harness = Some(harness(vec![response.to_string()]).0);
    app.start_comparison_turn("CHG-004");
    assert!(finish(&mut app));
    {
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
        let record = &project.state.workflow.plan_comparisons["CHG-004"];
        record.validate().unwrap();
        assert_eq!(
            record.status,
            crate::core::workflow::PlanComparisonStatus::Proposed
        );
        assert!(record.transcript.contains("Here are two approaches."));
        assert_eq!(record.alternatives.alternatives.len(), 2);
        assert_eq!(record.alternatives.recommendation.plan_id, "A");
        assert!(!saved.contains("## Plan Comparison"));
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
    let record = &project.state.workflow.plan_comparisons["CHG-004"];
    assert_eq!(
        record.status,
        crate::core::workflow::PlanComparisonStatus::Adopted
    );
    assert_eq!(record.selected_plan.as_deref(), Some("B"));
    let feature = &project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == "CHG-004")
        .unwrap()
        .1;
    assert_feature_metadata_has_no_plan_state(feature);
    if let Screen::Connected(project) = &mut app.screen {
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
#[test]
fn discard_keeps_the_comparison_in_private_workflow_state() {
    let _shield = crate::core::gitops::test_support::shield("feature-plan-discard");
    let (mut app, root, _) = setup();
    let store = match &app.screen {
        Screen::Connected(project) => project.state.planning_store.clone(),
        Screen::Welcome => panic!("fixture remains connected"),
    };
    let body = crate::artifacts::product_docs::current_document(&store, "feature:CHG-004")
        .unwrap()
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&body)
        .unwrap()
        .unwrap();
    let mut metadata = crate::domain::ChangeMetadata::require_markdown(&body).unwrap();
    metadata.schema_version = 2;
    let marker = format!(
        "<!-- koolade-change:v1 {} -->",
        serde_json::to_string(&metadata).unwrap()
    );
    let original_marker = body
        .lines()
        .find(|line| line.starts_with("<!-- koolade-change:v1 "))
        .unwrap();
    let body = body.replace(original_marker, &marker);
    let plan = |id: &str| crate::domain::PlanAlternative {
        id: id.into(),
        objective: format!("Plan {id}"),
        phases: vec![
            crate::domain::PlanPhase {
                name: "Prepare".into(),
                subtasks: vec!["Save state".into()]
            };
            3
        ],
        files_touched: vec![format!("src/{id}.rs")],
        state_changes: vec![format!("State {id}")],
        failure_modes: vec![format!("Failure {id}")],
        effort_band: "Small — one module".into(),
        known_risks: vec![format!("Risk {id}")],
        reversibility: format!("Undo {id}"),
    };
    let comparison = crate::domain::PlanComparison {
        alternatives: vec![plan("A"), plan("B")],
        recommendation: crate::domain::PlanRecommendation {
            plan_id: "A".into(),
            rationale: "Less risk".into(),
            evidence: vec!["src/A.rs".into()],
        },
        selected_plan: None,
    };
    assert_eq!(identity.display_id, "CHG-004");
    let changes = crate::core::apply::private_document_draft_changes(
        &store,
        &[("feature:CHG-004".into(), body)],
    )
    .unwrap();
    store.transaction(&changes, None).unwrap();
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
                transcript: "Two alternatives were compared; plan A was recommended.".into(),
                status: crate::core::workflow::PlanComparisonStatus::Proposed,
                selected_plan: None,
                updated_at_ms: 1,
            },
        );
        crate::artifacts::task_docs::save_workflow(&project.state.planning_store, &workflow)
            .unwrap();
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    app.dispatch(crate::ui::ApplicationCommand::DiscardFeaturePlans {
        id: "CHG-004".into(),
    });
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
    let store = match &app.screen {
        Screen::Connected(project) => &project.state.planning_store,
        Screen::Welcome => panic!("fixture remains connected"),
    };
    let saved = crate::artifacts::product_docs::current_document(store, "feature:CHG-004")
        .unwrap()
        .unwrap();
    assert_feature_metadata_has_no_plan_state(&saved);
    assert!(!saved.contains("## Plan Comparison History"));
    let workflow = crate::artifacts::task_docs::load_workflow(&private_store_root(&app)).unwrap();
    assert_eq!(
        workflow.plan_comparisons["CHG-004"].transcript,
        "Two alternatives were compared; plan A was recommended."
    );
    assert_eq!(
        workflow.plan_comparisons["CHG-004"]
            .alternatives
            .alternatives
            .len(),
        2
    );
    assert_eq!(
        workflow.plan_comparisons["CHG-004"].status,
        crate::core::workflow::PlanComparisonStatus::Discarded
    );
    assert_eq!(
        app.feature_actions(None)[0].label(),
        "Compare plans for CHG-004"
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
