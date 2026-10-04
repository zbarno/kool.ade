use super::*;

#[test]
fn saved_json_replies_render_as_readable_text_with_optional_diagnostics() {
    let mut app = fixture();
    let raw =
        r#"{"schema_version":1,"assistant_message":"Use corporate SSO.","open_items_updated":[]}"#;
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
                ChatMessage::new(
                    ChatRole::System,
                    "⚠ Turn rejected — nothing was written.\nInvalid update",
                    None,
                ),
                ChatMessage::new(ChatRole::Agent, raw, None),
            ],
        );
    }
    let ctx = egui::Context::default();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
    }
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Use corporate SSO.").is_some());
    assert!(text_position(&output, "Update not saved").is_some());
    click_text(&mut app, &ctx, "Which provider?");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Use corporate SSO.").is_some());
    assert!(
        text_position(
            &output,
            "Nothing changed. Retry your last reply or send a revised answer."
        )
        .is_some()
    );
    assert!(text_position(&output, raw).is_none());
    assert_eq!(app.task_messages("CLR-001")[1].text, raw);
    click_last(&mut app, &ctx, "Conversation history (2)");
    click_last(&mut app, &ctx, "Response details");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, raw).is_some());
}
