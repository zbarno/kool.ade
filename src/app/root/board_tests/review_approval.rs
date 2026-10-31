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

#[test]
fn verified_work_waiting_approval_is_in_review_with_direct_card_actions() {
    let mut app = fixture();
    let ticket = ".koolade-packet/planning/tasks/fixture/001-task.md";
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
    let ticket = ".koolade-packet/planning/tasks/fixture/001-task.md";
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

// Production modal -> explicit review response -> durable task history/resume
// boundary. Local Git fixture only; no provider or Docker worker is started.
// A painter-only test cannot establish that the mutable review route is used.
#[test]
fn requested_changes_have_an_explicit_response_separate_from_questions() {
    let mut app = fixture();
    let ticket = ".koolade-packet/planning/tasks/fixture/001-task.md";
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
