use super::*;

#[test]
fn hovering_a_task_highlights_its_batch_mates_and_leaves_unrelated_cards_alone() {
    let mut app = fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let batch_uid = uuid::Uuid::new_v4().to_string();
    for (index, doc) in project.task_documents.iter_mut().take(2).enumerate() {
        let mut identity =
            crate::domain::ArtifactIdentity::new(&format!("TASK-00{}", index + 1), &doc.title);
        identity.parent_uid = Some(batch_uid.clone());
        let metadata =
            crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", vec![]).unwrap();
        doc.identity = Some(identity);
        doc.metadata = Some(metadata);
    }

    let ctx = super::mockup_layout::styled_context();
    for _ in 0..4 {
        frame(&mut app, &ctx, vec![]);
    }
    let initial = frame(&mut app, &ctx, vec![]);
    let source = text_position(&initial, "First task").expect("source task card");
    let related = text_position(&initial, "Review task").expect("same-batch task card");
    let unrelated = text_position(&initial, "Merged task").expect("unrelated task card");

    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(source)]);
    let hovered = frame(&mut app, &ctx, vec![]);
    let blue_outline_contains = |position: egui::Pos2| {
        hovered.shapes.iter().any(|shape| {
            matches!(
                &shape.shape,
                egui::Shape::Rect(rect)
                    if rect.stroke.color == crate::ui::theme::BLUE_BRIGHT
                        && rect.rect.contains(position)
            )
        })
    };
    assert!(
        blue_outline_contains(source),
        "the hovered card gets a related-card outline"
    );
    assert!(
        blue_outline_contains(related),
        "its same-batch card is highlighted"
    );
    assert!(
        !blue_outline_contains(unrelated),
        "unrelated cards keep their normal outline"
    );
}

#[test]
fn attention_cards_distinguish_user_actions_from_external_retries() {
    let mut app = fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    project.queue.blocked.insert(
        project.task_documents[0].path.clone(),
        crate::core::implementation::Failure::new(
            crate::core::implementation::FailureKind::Other,
            crate::core::implementation::RecoveryDisposition::UserAction,
            "The account owner needs to update the access setting.",
        ),
    );
    project.queue.blocked.insert(
        project.task_documents[1].path.clone(),
        crate::core::implementation::Failure::new(
            crate::core::implementation::FailureKind::RemoteDiverged,
            crate::core::implementation::RecoveryDisposition::AutomaticRetry,
            "A remote update is being retried.",
        ),
    );
    let ctx = super::mockup_layout::styled_context();
    for _ in 0..4 {
        frame(&mut app, &ctx, vec![]);
    }
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Waiting on user").is_some());
    assert!(text_position(&output, "Blocked").is_some());
}

#[test]
fn actionable_feature_decision_is_shown_on_the_paused_task_and_clears_when_resolved() {
    let mut app = fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let path = ".koolade-packet/planning/tasks/CHG-012-feature/CHG-012-TASK-001-example.md";
    project.task_documents[0].path = path.into();
    project.queue.blocked.insert(
        path.into(),
        crate::core::implementation::Failure::new(
            crate::core::implementation::FailureKind::Other,
            crate::core::implementation::RecoveryDisposition::UserAction,
            "Answer the security decision before continuing.",
        ),
    );
    let mut item = crate::domain::item::OpenItem::new(
        "CLR-912".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        Some("All".into()),
        "Choose how long the session should remain active.".into(),
        "This decision gates the feature implementation.".into(),
    );
    item.feature_id = Some("CHG-012".into());
    project.state.items = vec![item];

    let ctx = super::mockup_layout::styled_context();
    for _ in 0..4 {
        frame(&mut app, &ctx, vec![]);
    }
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "NEEDS YOUR INPUT").is_some());
    assert!(text_position(&output, "Answer CLR-912").is_some());
    assert!(text_position(&output, "This feature is waiting on your decision.").is_some());
    assert!(text_position(&output, "Implementation failed").is_none());

    if let Screen::Connected(project) = &mut app.screen {
        project.state.items.clear();
    }
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Answer CLR-912").is_none());
    assert!(text_position(&output, "This feature is waiting on your decision.").is_none());
}

#[test]
fn todo_task_names_the_prerequisite_that_keeps_it_blocked() {
    let mut app = fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let batch_uid = uuid::Uuid::new_v4().to_string();
    let mut prerequisite = crate::domain::ArtifactIdentity::new("TASK-001", "First task");
    prerequisite.parent_uid = Some(batch_uid.clone());
    let mut dependent = crate::domain::ArtifactIdentity::new("TASK-002", "Review task");
    dependent.parent_uid = Some(batch_uid.clone());
    let prerequisite_metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&prerequisite, "root", vec![]).unwrap();
    let dependent_metadata = crate::artifacts::task_docs::TaskMetadata::new(
        &dependent,
        "root",
        vec![prerequisite.uid.clone()],
    )
    .unwrap();
    project.task_documents[0].identity = Some(prerequisite);
    project.task_documents[0].metadata = Some(prerequisite_metadata);
    project
        .implementation_states
        .remove(&project.task_documents[1].path);
    project.task_documents[1].identity = Some(dependent);
    project.task_documents[1].metadata = Some(dependent_metadata);

    let ctx = super::mockup_layout::styled_context();
    for _ in 0..4 {
        frame(&mut app, &ctx, vec![]);
    }
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Blocked").is_some());
    assert!(text_position(&output, "Waiting for First task").is_some());
}

#[test]
fn setup_attention_is_sent_to_the_manager_once_per_issue() {
    let mut app = fixture();
    app.setup_attention = Some(crate::app::setup_attention::SetupIssue::provider(
        "The saved provider configuration is unsupported.",
    ));
    app.queue_manager_setup_update();
    app.queue_manager_setup_update();

    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    let setup_events = project
        .activity
        .pending
        .iter()
        .filter(|event| event.contains("Workspace setup needs attention"))
        .collect::<Vec<_>>();
    assert_eq!(setup_events.len(), 1);
    assert!(setup_events[0].contains("Next action"));
}
