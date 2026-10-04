use super::*;

#[test]
fn task_story_discussion_updates_board_without_overriding_review_or_done() {
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        for doc in &p.task_documents {
            p.task_chats.messages.insert(
                doc.path.clone(),
                vec![
                    ChatMessage::new(ChatRole::User, "Use SSO", None),
                    ChatMessage::new(ChatRole::Agent, "Recorded.", None),
                ],
            );
        }
    }
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    for label in ["To do · 0", "In progress · 1", "In review · 1", "Done · 1"] {
        assert!(text_position(&output, label).is_some(), "missing {label}");
    }
    if let Screen::Connected(p) = &mut app.screen {
        p.task_chats
            .messages
            .get_mut(&p.task_documents[0].path)
            .unwrap()
            .push(ChatMessage::new(
                ChatRole::System,
                "Planning stopped: cancelled",
                None,
            ));
    }
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "In progress · 0").is_some());
    assert!(text_position(&output, "Needs attention · 1").is_some());
}
