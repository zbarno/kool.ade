use super::*;
#[test]
fn newest_typed_free_text_answer_supersedes_an_older_option() {
    let mut option = ChatMessage::new(ChatRole::User, "I choose option (old): Old plan.", None);
    option.answer_context = Some("current".into());
    let mut correction = ChatMessage::new(ChatRole::User, "My answer: Use the label Why?", None);
    correction.answer_context = Some("current".into());
    let context = latest_context_for(&[option, correction], "Morgan", Some("current")).unwrap();
    assert!(context.contains("Use the label Why?"));
    assert!(!context.contains("Old plan"));
}

#[test]
fn legacy_answers_are_reused_only_for_their_matching_attention_event() {
    let mut event = ChatMessage::new(ChatRole::Agent, "A saved question needs an answer.", None);
    event.id = "needs-attention:legacy".into();
    let answer = ChatMessage::new(
        ChatRole::User,
        "I choose option (keep): Keep the plan.",
        None,
    );
    let mut messages = vec![event, answer];
    assert!(latest_context_for(&messages, "Morgan", Some("needs-attention:legacy")).is_some());
    assert!(latest_context_for(&messages, "Morgan", Some("needs-attention:other")).is_none());
    let mut question = ChatMessage::new(ChatRole::User, "Please explain the consequences", None);
    question.clarification = true;
    messages.push(question);
    assert!(
        latest_context_for(&messages, "Morgan", Some("needs-attention:legacy"))
            .unwrap()
            .contains("Keep the plan")
    );
}

#[test]
fn a_clarification_statement_is_never_an_answer() {
    let mut question = ChatMessage::new(
        ChatRole::User,
        "I choose option (keep): explain that option",
        None,
    );
    question.clarification = true;
    question.answer_context = Some("current".into());
    assert!(latest_context_for(&[question], "Morgan", Some("current")).is_none());
}

#[test]
fn typed_answers_are_scoped_even_without_a_new_attention_message() {
    let mut answer = ChatMessage::new(ChatRole::User, "I choose option (keep): Keep it.", None);
    answer.answer_context = Some("old-blocker".into());
    let mut messages = vec![answer];
    assert!(latest_context_for(&messages, "Morgan", Some("new-blocker")).is_none());
    assert!(latest_context_for(&messages, "Morgan", Some("old-blocker")).is_some());
    messages.push(ChatMessage::new(ChatRole::User, "What happens next?", None));
    assert!(
        latest_context_for(&messages, "Morgan", Some("old-blocker"))
            .unwrap()
            .contains("Keep it")
    );
    let restored: Vec<ChatMessage> =
        serde_json::from_str(&serde_json::to_string(&messages).unwrap()).unwrap();
    assert_eq!(
        latest_context_for(&restored, "Morgan", Some("old-blocker")),
        latest_context_for(&messages, "Morgan", Some("old-blocker"))
    );
}

#[test]
fn a_new_blocker_does_not_reuse_a_previous_answer() {
    let messages = [
        ChatMessage::new(ChatRole::User, "I choose option (keep): Keep it.", None),
        ChatMessage::new(
            ChatRole::Agent,
            "## What Happened\nA different question needs an answer.",
            None,
        ),
    ];
    assert!(latest_context(&messages, "Morgan").is_none());
}

#[test]
fn clarification_does_not_replace_an_explicit_answer() {
    let messages = [
        ChatMessage::new(ChatRole::User, "I choose option (keep): Keep it.", None),
        ChatMessage::new(ChatRole::User, "What happens next?", None),
    ];
    let context = latest_context(&messages, "Morgan").unwrap();
    assert!(context.contains("I choose option (keep)"));
    assert!(!context.contains("What happens next?"));
}

#[test]
fn implementation_context_uses_only_the_latest_user_reply() {
    let messages = [
        ChatMessage::new(ChatRole::User, "old answer", None),
        ChatMessage::new(ChatRole::Agent, "clarification", None),
        ChatMessage::new(ChatRole::User, "I choose option (b)", None),
    ];
    let context = latest_context(&messages, "Morgan").unwrap();
    assert!(context.contains("Morgan"));
    assert!(context.contains("I choose option (b)"));
    assert!(!context.contains("old answer"));
}
