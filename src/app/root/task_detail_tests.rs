use super::*;

#[test]
fn task_details_read_from_one_model_and_emit_typed_commands() {
    let mut app = board_tests::fixture();
    let ticket = ".kool-ade-packet/planning/tasks/fixture/002-task.md";
    if let Screen::Connected(project) = &mut app.screen {
        project.task_chats.messages.insert(
            ticket.into(),
            vec![ChatMessage::new(
                ChatRole::Agent,
                "Which review option should we use?",
                Some(ticket.into()),
            )],
        );
        project
            .task_chats
            .drafts
            .insert(ticket.into(), "Keep the existing approach".into());
    }

    let view = app.task_detail_view(ticket).expect("connected task model");
    assert_eq!(
        view.implementation.as_ref().map(|state| state.status),
        Some(ImplementationStatus::AwaitingReview)
    );
    assert_eq!(view.board_column, 2);
    assert!(!view.implementation_active);
    assert_eq!(view.messages.len(), 1);
    assert_eq!(view.draft, "Keep the existing approach");

    app.dispatch_ui_command(crate::ui::ApplicationCommand::TaskDetail(
        crate::ui::task_detail::Command::UpdateDraft {
            ticket: ticket.into(),
            draft: "Use the existing approach and document why.".into(),
        },
    ));
    app.dispatch_ui_command(crate::ui::ApplicationCommand::TaskDetail(
        crate::ui::task_detail::Command::StopAndPause {
            ticket: ticket.into(),
        },
    ));

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected");
    };
    assert_eq!(
        project.task_chats.drafts.get(ticket).map(String::as_str),
        Some("Use the existing approach and document why.")
    );
    assert!(!project.queue.running);
    assert!(project.queue.recovery_paused);
}
