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
        "CURRENT STATE",
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
