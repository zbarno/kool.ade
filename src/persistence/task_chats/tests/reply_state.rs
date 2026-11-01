use super::*;

#[test]
fn manager_index_keeps_clarification_questions_separate_from_submitted_answers() {
    let key = "feature:F1";
    let mut chats = TaskChats::default();
    let answer = ChatMessage::new(ChatRole::User, "Keep SSO as the default", None);
    let mut clarification = ChatMessage::new(ChatRole::User, "What does SSO mean?", None);
    clarification.clarification = true;
    let mut clarification_reply = ChatMessage::new(ChatRole::Agent, "Single sign-on.", None);
    clarification_reply.clarification = true;
    chats.messages.insert(
        key.into(),
        vec![
            answer,
            clarification.clone(),
            clarification_reply,
            ChatMessage::new(ChatRole::System, "No project changes were applied.", None),
        ],
    );

    let context = chats.project_context("F1", 24000);
    let index = context.split("Conversation index").nth(1).unwrap();
    let index = index.split("Indexed ").next().unwrap();
    assert!(index.contains("submitted answer: Keep SSO as the default"));
    assert!(index.contains("clarification-only question: What does SSO mean?"));
    assert!(!index.contains("submitted answer: What does SSO mean?"));
}

#[test]
fn pending_user_reply_is_detected_until_an_agent_answers() {
    let key = "CLR-001";
    let mut chats = TaskChats::default();
    chats.messages.insert(
        key.into(),
        vec![ChatMessage::new(
            ChatRole::User,
            "Use release/2.1.",
            Some(key.into()),
        )],
    );
    assert!(chats.has_pending_user_reply(key));
    chats.messages.get_mut(key).unwrap().push(ChatMessage::new(
        ChatRole::Agent,
        "I recorded that choice.",
        None,
    ));
    assert!(!chats.has_pending_user_reply(key));
}

/// Production boundary: durable task-reply claim storage shared by independent windows. This proves one process cannot append a competing answer while another reply is claimed; an in-memory unit test cannot exercise the store lock and persisted claim. Uses a temporary local filesystem, no external services, low setup cost.
#[test]
fn uid_task_reply_claim_blocks_a_second_window_from_saving_a_new_answer() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_uid_reply_claim_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let slug = dir.to_str().unwrap();
    let aliases = [("feature:F1".into(), "feature-uid".into())];
    let mut first = TaskChats::default();
    first.bind_conversation_identities(&aliases, &[]).unwrap();
    first.ensure_loaded(slug);
    let user = ChatMessage::new(ChatRole::User, "Keep the current behavior", None);
    first
        .append_user_reply_and_claim(slug, "feature:F1", user)
        .unwrap();

    let mut second = TaskChats::default();
    second.bind_conversation_identities(&aliases, &[]).unwrap();
    second.ensure_loaded(slug);
    assert!(
        second
            .append(
                slug,
                "feature:F1",
                vec![ChatMessage::new(
                    ChatRole::User,
                    "Change the behavior",
                    None
                )],
            )
            .is_err()
    );
    std::fs::remove_dir_all(dir).unwrap();
}
