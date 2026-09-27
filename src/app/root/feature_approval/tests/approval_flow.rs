use super::*;

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
        root.join(".kool-ade-packet/planning/changes/CHG-004-saved-searches/specification.md");
    let mut markdown = std::fs::read_to_string(&path).unwrap();
    let marker = markdown
        .lines()
        .find(|line| line.starts_with("<!-- packet-change:v1 "))
        .unwrap()
        .to_owned();
    let json = marker
        .strip_prefix("<!-- packet-change:v1 ")
        .unwrap()
        .strip_suffix(" -->")
        .unwrap();
    let mut metadata: serde_json::Value = serde_json::from_str(json).unwrap();
    metadata["planComparison"]["selected_plan"] = serde_json::Value::Null;
    markdown = markdown.replace(
        &marker,
        &format!(
            "<!-- packet-change:v1 {} -->",
            serde_json::to_string(&metadata).unwrap()
        ),
    );
    std::fs::write(&path, markdown).unwrap();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    assert_eq!(
        app.feature_actions(None)[0].label(),
        "Choose a plan for CHG-004 below"
    );
    app.approve_feature_only("CHG-004");
    assert!(!app.feature_approved("CHG-004"));

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
    assert_eq!(
        saved.plan_comparison.unwrap().selected_plan.as_deref(),
        Some("B")
    );
    app.approve_feature_only("CHG-004");
    assert!(app.feature_approved("CHG-004"));
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn approval_click_refreshes_stale_review_and_generates_stories_without_second_approval() {
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
    assert!(matches!(&app.screen, Screen::Connected(project) if !project.queue.auto_publish));
    assert!(
        app.task_messages("CLR-026")
            .last()
            .unwrap()
            .text
            .contains("Approved CHG-004")
    );
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
    let applied = finish(&mut app);
    assert!(!applied);
    app.continue_feature_generation(applied);
    assert!(app.feature_approved("CHG-004"));
    assert!(app.pending_feature_generation.is_none());
    assert_eq!(
        app.feature_actions(Some("CLR-026"))[0].label(),
        "Prepare tasks for CHG-004"
    );
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Prepare tasks for CHG-004").is_some());
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
