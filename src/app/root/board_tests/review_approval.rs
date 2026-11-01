use super::*;

fn awaiting_approval(app: &mut KooladeApp, ticket: &str) {
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!();
    };
    let mut record = project
        .implementation_states
        .values()
        .next()
        .unwrap()
        .clone();
    record.ticket = ticket.to_owned();
    record.ticket_text = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
        .unwrap()
        .text
        .clone();
    record.status = ImplementationStatus::AwaitingApproval;
    record.pr_url = None;
    record.pr_state = None;
    project
        .implementation_states
        .insert(ticket.to_owned(), record);
}

fn make_task_identity_ambiguous(project: &mut crate::app::session::Project) {
    let mut first = crate::domain::ArtifactIdentity::new("TASK-001", "First task");
    first.uid = uuid::Uuid::new_v4().to_string();
    let mut second = crate::domain::ArtifactIdentity::new("TASK-001", "First task");
    second.uid = uuid::Uuid::new_v4().to_string();
    project.task_documents[0].identity = Some(first);
    let mut duplicate = project.task_documents[0].clone();
    duplicate.identity = Some(second);
    project.task_documents.push(duplicate);
}

#[test]
fn verified_work_waiting_approval_is_in_review_with_direct_card_actions() {
    let mut app = fixture();
    let ticket = "planning/tasks/fixture/001-task.md";
    awaiting_approval(&mut app, ticket);
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = click_text(&mut app, &ctx, "First task");
    for expected in [
        "Why am I seeing this?",
        "Implementation is complete and verified",
        "Approve and create pull request",
        "Request changes",
    ] {
        assert!(text_contains(&output, expected), "missing {expected}");
    }
    assert!(text_position(&output, "Interrupted — no worker is running").is_none());
    if let Screen::Connected(project) = &app.screen {
        assert_eq!(
            crate::core::implementation::board_column(
                project.implementation_states.get(ticket),
                false
            ),
            2
        );
    }
}

#[test]
fn changes_requested_is_saved_as_a_review_outcome() {
    let mut app = fixture();
    let ticket = "planning/tasks/fixture/001-task.md";
    awaiting_approval(&mut app, ticket);

    app.dispatch_ui_command(crate::ui::ApplicationCommand::RequestPublicationChanges {
        ticket: ticket.to_owned(),
    });

    if let Screen::Connected(project) = &app.screen {
        assert_eq!(
            project.implementation_states[ticket].status,
            ImplementationStatus::ChangesRequested
        );
        assert!(project.task_chats.messages[ticket].iter().any(|message| {
            message.role == ChatRole::User
                && message.text == "I am requesting changes before approving a pull request."
        }));
    }
}

#[test]
fn ambiguous_task_identity_preserves_requested_changes_draft() {
    let mut app = fixture();
    let ticket = "planning/tasks/fixture/001-task.md";
    if let Screen::Connected(project) = &mut app.screen {
        make_task_identity_ambiguous(project);

        let mut record = project
            .implementation_states
            .values()
            .next()
            .unwrap()
            .clone();
        record.ticket = ticket.into();
        record.status = ImplementationStatus::ChangesRequested;
        project.implementation_states.insert(ticket.into(), record);
        project
            .task_chats
            .drafts
            .insert(ticket.into(), "Improve the empty state.".into());
    }

    app.submit_task_reply(ticket);

    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    assert_eq!(
        project.task_chats.drafts.get(ticket).map(String::as_str),
        Some("Improve the empty state.")
    );
    assert!(!project.task_chats.messages.contains_key(ticket));
    assert!(!project.active_implementations.contains_key(ticket));
}

/// Production action boundary: every task mutation must refuse a display key
/// that maps to duplicate immutable artifact identities. Conversation-only
/// storage tests cannot prove review state and implementation workers remain
/// untouched. Uses the local board fixture; no provider, Docker, SSH, or
/// network dependencies; low setup cost.
#[test]
fn ambiguous_identity_blocks_review_decision_and_worker_start() {
    let mut app = fixture();
    let ticket = "planning/tasks/fixture/001-task.md";
    awaiting_approval(&mut app, ticket);
    if let Screen::Connected(project) = &mut app.screen {
        make_task_identity_ambiguous(project);
        project.queue.blocked.insert(
            ticket.into(),
            crate::core::implementation::Failure::other("Needs a decision"),
        );
        project
            .task_chats
            .drafts
            .insert(ticket.into(), "Keep the verified result.".into());
    }

    app.request_publication_changes(ticket);
    app.approve_publication(ticket);
    assert!(!app.submit_implementation_decision(ticket));
    app.start_implementation(ticket.into(), true);

    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    assert_eq!(
        project.implementation_states[ticket].status,
        ImplementationStatus::AwaitingApproval
    );
    assert_eq!(
        project.task_chats.drafts.get(ticket).map(String::as_str),
        Some("Keep the verified result.")
    );
    assert!(!project.task_chats.messages.contains_key(ticket));
    assert!(!project.active_implementations.contains_key(ticket));
}

// Production modal -> explicit review response -> durable task history/resume
// boundary. Local Git fixture only; no provider or Docker worker is started.
// A painter-only test cannot establish that the mutable review route is used.
#[test]
fn requested_changes_have_an_explicit_response_separate_from_questions() {
    let mut app = fixture();
    let ticket = "planning/tasks/fixture/001-task.md";
    awaiting_approval(&mut app, ticket);
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "First task");
    let output = click_text(&mut app, &ctx, "Request changes");
    assert!(text_contains(&output, "Action needed"));
    click_text(&mut app, &ctx, "Describe the changes…");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("Improve the empty state.".into())],
    );
    click_text(&mut app, &ctx, "Submit changes");
    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    assert!(project.task_chats.messages[ticket].iter().any(|message| {
        message.role == ChatRole::User
            && message.text == "Improve the empty state."
            && !message.clarification
    }));
    assert!(
        project
            .activity
            .pending
            .iter()
            .any(|event| event.contains("Requested changes for task")
                && event.contains("Improve the empty state."))
    );
    assert!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new(("task_answer", ticket))))
            .unwrap_or_default()
            .is_empty()
    );
}
