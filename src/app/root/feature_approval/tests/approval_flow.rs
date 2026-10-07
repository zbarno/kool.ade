use super::*;

#[path = "approval_flow/concurrent_generation.rs"]
mod concurrent_generation;

fn compact_story(source: &str, picker: bool) -> String {
    let mut response: serde_json::Value = serde_json::from_str(source).unwrap();
    let story = &mut response["task_stories"][0];
    if picker {
        story["intent"] = "Analysts cannot select saved filters from the search UI.".into();
        story["goal"] = "Offer a keyboard-accessible picker for saved filters.".into();
        story["context"] = "Use saved records and preserve ad hoc search behavior.".into();
        story["user_story"] = "As an analyst, I want to select a saved filter.".into();
        story["affected_files"] =
            serde_json::json!(["src/ui/search_picker.rs: add saved-filter selection."]);
        story["implementation_steps"] = serde_json::json!([
            "List saved filters and restore a selection with recoverable errors."
        ]);
        story["acceptance_criteria"] = serde_json::json!([
            "Keyboard selection restores the chosen filter.",
            "A missing record leaves the current search unchanged."
        ]);
        story["test_plan"] =
            serde_json::json!(["Test keyboard selection and missing-record recovery."]);
        story["verification_commands"] = serde_json::json!(["cargo test search_picker"]);
    } else {
        story["intent"] = "Saved filters are lost when analysts restart.".into();
        story["goal"] = "Persist named filters and restore complete records.".into();
        story["context"] = "Use the existing filter model; do not store rendered results.".into();
        story["user_story"] = "As an analyst, I want saved filters after restart.".into();
        story["affected_files"] =
            serde_json::json!(["src/search_store.rs: add local saved-filter storage."]);
        story["implementation_steps"] =
            serde_json::json!(["Add versioned atomic load and save for complete filter records."]);
        story["acceptance_criteria"] = serde_json::json!([
            "Reload restores a saved filter exactly.",
            "A failed write preserves the previous store."
        ]);
        story["test_plan"] =
            serde_json::json!(["Test a complete round trip and one injected write failure."]);
        story["verification_commands"] = serde_json::json!(["cargo test search_store"]);
    }
    story["technical_design"] = serde_json::json!([]);
    story["edge_cases"] = serde_json::json!([]);
    story["rollout_notes"] = "".into();
    story["definition_of_done"] = serde_json::json!(["Behavior and focused tests pass."]);
    response.to_string()
}

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
    }
    assert_eq!(
        app.feature_actions(None)[0].label(),
        "Approve CHG-004 and prepare tasks"
    );
    app.approve_feature_only("CHG-004");
    assert!(
        app.feature_approved("CHG-004"),
        "{:?}",
        app.chat_messages().last()
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn compare_plans_turn_persists_two_options_for_the_ready_feature() {
    let _shield = crate::core::gitops::test_support::shield("feature-plan-comparison-turn");
    let (mut app, root, _) = setup();
    let path =
        root.join(".koolade-packet/planning/changes/CHG-004-saved-searches/specification.md");
    let body = std::fs::read_to_string(&path).unwrap();
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
    std::fs::write(
        &path,
        body.replace(
            marker,
            &format!(
                "<!-- koolade-change:v1 {} -->",
                serde_json::to_string(&metadata).unwrap()
            ),
        ),
    )
    .unwrap();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
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
    let metadata = crate::domain::ChangeMetadata::require_markdown(feature).unwrap();
    assert!(metadata.selected_alt.is_none());
    assert!(metadata.plan_comparison.is_none());
    app.approve_feature_only("CHG-004");
    assert!(
        app.feature_approved("CHG-004"),
        "{:?}",
        app.chat_messages().last()
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn discard_recompare_action_keeps_the_previous_plan_transcript() {
    let _shield = crate::core::gitops::test_support::shield("feature-plan-discard");
    let (mut app, root, _) = setup();
    let path =
        root.join(".koolade-packet/planning/changes/CHG-004-saved-searches/specification.md");
    let body = std::fs::read_to_string(&path).unwrap();
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
    let compared = crate::domain::ChangeMetadata::save_plan_comparison(
        &body,
        crate::domain::PlanComparison {
            alternatives: vec![plan("A"), plan("B")],
            recommendation: crate::domain::PlanRecommendation {
                plan_id: "A".into(),
                rationale: "Less risk".into(),
                evidence: vec!["src/A.rs".into()],
            },
            selected_plan: None,
        },
    )
    .unwrap();
    assert_eq!(identity.display_id, "CHG-004");
    std::fs::write(&path, compared).unwrap();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    app.dispatch(crate::ui::ApplicationCommand::DiscardFeaturePlans {
        id: "CHG-004".into(),
    });
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    let saved = std::fs::read_to_string(path).unwrap();
    let metadata = crate::domain::ChangeMetadata::require_markdown(&saved).unwrap();
    assert!(metadata.plan_comparison.is_none());
    assert!(metadata.comparison_history.is_empty());
    assert!(saved.contains("## Plan Comparison History"));
    let workflow = crate::artifacts::task_docs::load_workflow(&root).unwrap();
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

#[test]
fn board_approval_adds_a_task_generation_card_that_generates_stories_on_request() {
    let _shield = crate::core::gitops::test_support::shield("feature-approval");
    let (mut app, root, review) = setup();
    assert!(
        app.task_offer().is_none(),
        "reproduce stale review hiding the old offer"
    );
    let (h, calls) = harness(vec![review.to_string()]);
    app.task_harness = Some(h);
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Approve CHG-004 and prepare tasks").is_some());
    click_text(&mut app, &ctx, "Approve CHG-004 and prepare tasks");
    assert!(app.feature_approved("CHG-004"));
    let generation_key = match &app.screen {
        Screen::Connected(project) => project
            .planning_work
            .iter()
            .find(|work| work.kind == crate::core::planning_work::WorkKind::TaskGeneration)
            .map(|work| work.key.clone())
            .expect("approval creates a board task-generation card"),
        _ => panic!("project remains connected"),
    };
    assert!(matches!(&app.screen, Screen::Connected(project) if !project.queue.auto_publish));
    assert!(
        app.task_messages("CLR-026")
            .last()
            .unwrap()
            .text
            .contains("Approved CHG-004")
    );
    assert!(matches!(&app.screen, Screen::Connected(project) if project.active_turn.is_none()));
    app.start_task_generation(&generation_key, "CHG-004");
    let applied = finish(&mut app);
    assert!(applied, "{:?}", app.chat_messages());
    assert!(calls.lock().unwrap()[0].contains("do not ask for approval again"));
    let (h, generated) = harness(vec![
        include_str!("../../../../../tests/fixtures/task-outline.json").into(),
        compact_story(
            include_str!("../../../../../tests/fixtures/task-story-1.json"),
            false,
        ),
        compact_story(
            include_str!("../../../../../tests/fixtures/task-story-2.json"),
            true,
        ),
    ]);
    app.task_harness = Some(h);
    app.continue_feature_generation(applied);
    assert!(finish(&mut app), "{:?}", app.chat_messages());
    assert_eq!(generated.lock().unwrap().len(), 3);
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    assert!(has_current_task_batch(p));
    assert!(
        p.task_documents
            .iter()
            .any(|doc| doc.text.contains("Feature ID: CHG-004"))
    );
    assert!(app.feature_actions(Some("CLR-026")).is_empty());
    assert!(app.implementation_offer());
    assert!(matches!(&app.screen, Screen::Connected(project) if !project.queue.auto_publish));
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn resolved_conversation_has_action_and_failed_review_retains_approval_for_retry() {
    let _shield = crate::core::gitops::test_support::shield("feature-approval-retry");
    let (mut app, root, _) = setup();
    let (h, _) = harness(vec!["malformed response".into()]);
    app.task_harness = Some(h);
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Open conversation");
    click_text(&mut app, &ctx, "Approve CHG-004 and prepare tasks");
    let generation_key = match &app.screen {
        Screen::Connected(project) => project
            .planning_work
            .iter()
            .find(|work| work.kind == crate::core::planning_work::WorkKind::TaskGeneration)
            .map(|work| work.key.clone())
            .expect("approval creates a board task-generation card"),
        _ => panic!("project remains connected"),
    };
    app.start_task_generation(&generation_key, "CHG-004");
    let applied = finish(&mut app);
    assert!(!applied);
    app.continue_feature_generation(applied);
    assert!(app.feature_approved("CHG-004"));
    assert!(app.pending_feature_generation.is_none());
    assert!(
        matches!(&app.screen, Screen::Connected(project) if project.planning_work.iter().any(|work| work.key == generation_key && work.status == crate::core::planning_work::WorkStatus::NeedsAttention))
    );
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Generate tasks").is_some());
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
