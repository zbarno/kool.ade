use super::*;

#[test]
fn next_step_is_prominent_and_older_messages_are_disclosed_on_request() {
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        p.task_documents.clear();
        p.state.items = vec![OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::High,
            crate::domain::ItemKind::Question,
            "General".into(),
            None,
            "Which provider?".into(),
            "Access".into(),
        )];
        p.task_chats.messages.insert(
            "CLR-001".into(),
            vec![
                ChatMessage::new(ChatRole::User, "Earlier context for the provider", None),
                ChatMessage::new(ChatRole::Agent, "Which authentication method?", None),
                ChatMessage::new(ChatRole::User, "Use corporate SSO", None),
                ChatMessage::new(
                    ChatRole::Agent,
                    "SSO is recorded.\nYour next step: Should guests use SSO too?",
                    None,
                ),
            ],
        );
    }
    let ctx = egui::Context::default();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
    }
    frame(&mut app, &ctx, vec![]);
    let output = click_text(&mut app, &ctx, "Which provider?");
    assert!(text_position(&output, "Task conversation").is_some());
    assert!(text_position(&output, "Should guests use SSO too?").is_some());
    assert!(text_position(&output, "Reply about this task…").is_some());
    assert!(text_position(&output, "Ctrl + Enter to send").is_some());
    assert!(text_position(&output, "Earlier context for the provider").is_some());
}
