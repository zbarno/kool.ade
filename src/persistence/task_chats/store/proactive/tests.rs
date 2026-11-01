use super::*;

#[test]
/// Production boundary: peer replies and proactive prompts share the durable
/// task-conversation lock. This reproduces the check/write interleaving that a
/// helper unit test cannot prove; it uses temporary local files only.
fn a_peer_answer_between_attention_check_and_save_suppresses_the_prompt() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_proactive_race_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let slug = dir.to_str().unwrap();
    let key = "CLR-038";
    let identities = [(key.into(), "clr-038-uid".into())];
    let mut attention_window = TaskChats::default();
    attention_window
        .bind_conversation_identities(&identities, &[])
        .unwrap();
    attention_window.ensure_loaded(slug);

    let mut peer_window = TaskChats::default();
    peer_window
        .bind_conversation_identities(&identities, &[])
        .unwrap();
    peer_window.ensure_loaded(slug);
    peer_window
        .append_user_reply_and_claim(
            slug,
            key,
            ChatMessage::new(ChatRole::User, "Use release/2.1.", Some(key.into())),
        )
        .unwrap();

    let prompt = ChatMessage::new(
        ChatRole::Agent,
        "Which branch should receive this task?",
        Some(key.into()),
    );
    assert!(
        !attention_window
            .remember_proactive_response_if_unanswered(slug, key, vec![prompt.clone()])
            .unwrap()
    );
    assert!(
        attention_window.messages[key]
            .iter()
            .any(|message| message.text == "Use release/2.1.")
    );
    assert!(
        attention_window.messages[key]
            .iter()
            .all(|message| message.id != prompt.id)
    );

    let mut reopened = TaskChats::default();
    reopened
        .bind_conversation_identities(&identities, &[])
        .unwrap();
    reopened.ensure_loaded(slug);
    assert!(
        reopened.messages[key]
            .iter()
            .all(|message| message.id != prompt.id)
    );
    std::fs::remove_dir_all(dir).unwrap();
}
