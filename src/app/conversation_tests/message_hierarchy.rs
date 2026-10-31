use super::*;
use crate::app::root::board_tests::text_contains;

#[test]
fn task_conversation_preserves_the_open_question_and_earlier_context() {
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
    assert!(text_position(&output, "Ask a question").is_some());
    assert!(text_contains(&output, "Should guests use SSO too?"));
    assert!(text_position(&output, "Ask for clarification…").is_some());
    assert!(text_position(&output, "Send").is_some());
    assert!(text_position(&output, "Earlier context for the provider").is_some());
}
