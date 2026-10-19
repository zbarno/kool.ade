use super::*;

#[path = "approval_flow/concurrent_generation.rs"]
mod concurrent_generation;
#[path = "approval_flow/stale_approval.rs"]
mod stale_approval;

#[path = "approval_flow/clarification_continuation.rs"]
mod clarification_continuation;
#[path = "approval_flow/compare.rs"]
mod compare;
#[path = "approval_flow/plan_choice.rs"]
mod plan_choice;
#[path = "approval_flow/request_changes.rs"]
mod request_changes;

#[test]
fn board_approval_adds_a_task_generation_card_that_generates_stories_on_request() {
    // Production boundary: Board card selection opens the feature's Task Details conversation surface.
    // Invariant: plan confirmation stays on the existing feature thread and its explicit action remains visible; a command-level approval test cannot prove the rendered Board-to-details route.
    // External dependencies: none. Setup cost: one temporary Git project plus egui frames.
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
    assert!(text_position(&output, "CHG-004: Saved searches").is_some());
    click_text(&mut app, &ctx, "Review specification");
    let details = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&details, "Task conversation").is_some());
    assert!(text_position(&details, "Request changes").is_some());
    click_text(&mut app, &ctx, "Confirm this plan");
    record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    app.approve_feature_from_board("CHG-004");
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
            .contains("Plan confirmation recorded for CHG-004")
    );
    assert!(matches!(&app.screen, Screen::Connected(project) if project.active_turn.is_none()));
    app.start_task_generation(&generation_key, "CHG-004");
    let applied = finish(&mut app);
    assert!(applied, "{:?}", app.chat_messages());
    assert!(
        app.feature_approved("CHG-004"),
        "review receipts should survive the preparation turn"
    );
    assert!(calls.lock().unwrap()[0].contains("Do not ask for approval again"));
    let (h, generated) = harness(vec![decomposer_graph(&app)]);
    app.task_harness = Some(h);
    app.continue_feature_generation(applied);
    assert!(finish(&mut app), "{:?}", app.chat_messages());
    assert_eq!(generated.lock().unwrap().len(), 1);
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    assert!(has_current_task_batch(p.as_ref()));
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
    click_text(&mut app, &ctx, "Review specification");
    click_text(&mut app, &ctx, "Confirm this plan");
    record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    app.approve_feature_from_board("CHG-004");
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
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Generate tasks: CHG-004: Saved searches");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(
        text_position(&output, "Generate tasks").is_some(),
        "{:?}",
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
