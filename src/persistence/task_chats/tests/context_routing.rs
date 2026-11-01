use super::*;

#[test]
fn detached_histories_are_not_projected_into_manager_context() {
    let mut chats = TaskChats::default();
    chats.messages.insert(
        "@legacy:feature:F1".into(),
        vec![ChatMessage::new(
            ChatRole::User,
            "Prior feature answer",
            None,
        )],
    );
    chats.messages.insert(
        "@unlinked:prior-task-uid".into(),
        vec![ChatMessage::new(ChatRole::User, "Prior task answer", None)],
    );

    assert!(chats.project_context("", 24000).is_empty());
}

#[test]
fn focused_context_does_not_match_a_longer_display_id() {
    let mut chats = TaskChats::default();
    chats.messages.insert(
        "feature:F1".into(),
        vec![ChatMessage::new(ChatRole::User, "F1 feature history", None)],
    );
    chats.messages.insert(
        "feature:F10".into(),
        vec![ChatMessage::new(
            ChatRole::User,
            "F10 feature history",
            None,
        )],
    );

    let focused = chats.project_context_for_focus("Please reuse feature:F10 history", 24000);
    assert!(focused.contains("F10 feature history"));
    assert!(!focused.contains("F1 feature history"));

    let focused = chats.project_context_for_focus("Investigate F1", 24000);
    assert!(focused.contains("F1 feature history"));
    assert!(!focused.contains("F10 feature history"));
}
