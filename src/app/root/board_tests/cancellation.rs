use super::*;

#[test]
fn board_cancel_requires_a_visible_confirmation() {
    let mut app = fixture();
    let ticket = ".koolade-packet/planning/tasks/fixture/001-task.md";
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let dialog = click_text(&mut app, &ctx, "Cancel");
    assert!(text_position(&dialog, "Confirm cancel").is_some());
    assert!(text_contains(&dialog, "execution queue"));
    click_text(&mut app, &ctx, "Confirm cancel");
    let Screen::Connected(project) = &app.screen else {
        panic!("disconnected");
    };
    assert!(project.task_cancelled(ticket));
}

#[test]
fn queued_running_attention_review_and_completed_tasks_cancel_safely() {
    let mut app = fixture();
    let root = std::env::temp_dir().join(format!(
        "koolade-cancel-board-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let Screen::Connected(project) = &mut app.screen else {
        panic!("disconnected");
    };
    project.state = crate::core::state::PlannerState::load(&root).unwrap();
    let queued = project.task_documents[0].path.clone();
    let review = project.task_documents[1].path.clone();
    let complete = project.task_documents[2].path.clone();
    let attention = ".koolade-packet/planning/tasks/fixture/004-attention.md".to_owned();
    let mut attention_doc = project.task_documents[0].clone();
    attention_doc.path = attention.clone();
    attention_doc.title = "Needs attention task".into();
    project.task_documents.push(attention_doc);
    let mut attention_state = project.implementation_states[&review].clone();
    attention_state.ticket = attention.clone();
    attention_state.ticket_text = "# Needs attention task\n".into();
    attention_state.status = ImplementationStatus::Blocked;
    attention_state.pr_url = None;
    project
        .implementation_states
        .insert(attention.clone(), attention_state);
    project.queue.current_ticket = Some(queued.clone());
    project.queue.in_flight.insert(queued.clone());
    project.queue.recovery_attempts.insert(queued.clone(), 1);
    project.queue.blocked.insert(
        queued.clone(),
        crate::core::implementation::Failure::other("queued blocker"),
    );
    project.queue.blocked.insert(
        review.clone(),
        crate::core::implementation::Failure::other("review blocker"),
    );
    let running_controller = crate::core::implementation::Controller::idle_fixture();
    project
        .active_implementations
        .insert(queued.clone(), running_controller);
    project
        .implementation_states
        .get_mut(&review)
        .unwrap()
        .status = ImplementationStatus::AwaitingReview;

    app.dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork {
        key: queued.clone(),
    });
    app.dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork {
        key: queued.clone(),
    });

    {
        let Screen::Connected(project) = &app.screen else {
            panic!("disconnected");
        };
        assert!(project.task_cancelled(&queued));
        assert!(project.active_implementations[&queued].cancellation_requested());
        assert!(!project.queue.in_flight.contains(&queued));
        assert!(!project.queue.blocked.contains_key(&queued));
        assert!(!project.queue.recovery_attempts.contains_key(&queued));
        assert_eq!(
            crate::persistence::cancelled_work::load(&root).unwrap(),
            project.cancelled_work
        );
    }
    if let Screen::Connected(project) = &mut app.screen {
        project.active_implementations.remove(&queued);
    }
    let available = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
    app.start_implementation_with_capabilities(queued.clone(), true, available);
    let Screen::Connected(project) = &app.screen else {
        panic!("disconnected");
    };
    assert!(
        !project.active_implementations.contains_key(&queued),
        "cancelled work cannot resume"
    );

    app.dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork {
        key: review.clone(),
    });
    app.dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork {
        key: attention.clone(),
    });
    app.dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork {
        key: complete.clone(),
    });
    let Screen::Connected(project) = &app.screen else {
        panic!("disconnected");
    };
    assert!(project.task_cancelled(&review));
    assert!(project.task_cancelled(&attention));
    assert!(
        !project.task_cancelled(&complete),
        "completed tasks cannot become cancelled"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelling_a_feature_cancels_unfinished_children_but_keeps_completed_children() {
    let mut app = fixture();
    let root = std::env::temp_dir().join(format!(
        "koolade-cancel-feature-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let feature_md = crate::domain::ArtifactIdentity::preserve_markdown(
        "# CHG-001: Parent feature\n\nFeature description\n",
        None,
        "CHG-001",
        "Parent feature",
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&feature_md)
        .unwrap()
        .unwrap();
    let feature = crate::domain::ChangeMetadata::write_markdown(
        &feature_md,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("disconnected");
    };
    project.state = crate::core::state::PlannerState::load(&root).unwrap();
    project
        .state
        .active_features
        .push(("CHG-001".into(), feature));
    let mut work = crate::core::planning_work::Work::new(
        "planning:parent".into(),
        "Plan Parent feature".into(),
        "request".into(),
        "review".into(),
    );
    work.feature_id = Some("CHG-001".into());
    work.feature_uid = Some(identity.uid.clone());
    let key = work.key.clone();
    project.planning_work.push(work.clone());
    let unfinished = project.task_documents[0].clone();
    let completed = project.task_documents[2].clone();
    for doc in [&unfinished, &completed] {
        let task = project
            .task_documents
            .iter_mut()
            .find(|task| task.path == doc.path)
            .unwrap();
        task.text.push_str("\nFeature ID: CHG-001\n");
    }
    app.dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork { key: key.clone() });
    app.dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork { key });
    let Screen::Connected(project) = &app.screen else {
        panic!("disconnected");
    };
    assert!(
        project
            .cancelled_work
            .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
    );
    assert!(project.task_cancelled(&unfinished.path));
    assert!(!project.task_cancelled(&completed.path));
    std::fs::remove_dir_all(root).unwrap();
}
