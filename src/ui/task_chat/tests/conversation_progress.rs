use crate::domain::{ChatMessage, ChatRole};
use crate::ui::task_chat::{Reply, board_column, split_reply};
#[test]
fn conversation_progress_preserves_explicit_lifecycle_states() {
    let mut messages = vec![];
    assert_eq!(board_column(0, &messages, false), 0);
    messages.push(ChatMessage::new(ChatRole::User, "Use SSO", None));
    assert_eq!(board_column(0, &messages, true), 1);
    assert_eq!(board_column(0, &messages, false), 3); // Interrupted reply.
    messages.push(ChatMessage::new(
        ChatRole::Agent,
        "Recorded. Anything else?",
        None,
    ));
    assert_eq!(board_column(0, &messages, false), 1);
    // Saved history retains progress after a restart, without marking Done.
    let restored: Vec<ChatMessage> =
        serde_json::from_str(&serde_json::to_string(&messages).unwrap()).unwrap();
    assert_eq!(board_column(0, &restored, false), 1);
    for base in [2, 3, 4] {
        assert_eq!(board_column(base, &restored, false), base);
        assert_eq!(board_column(base, &restored, true), base);
    }
    messages.push(ChatMessage::new(
        ChatRole::System,
        "Planning stopped: provider unavailable",
        None,
    ));
    assert_eq!(board_column(0, &messages, false), 3);
    assert_eq!(board_column(0, &messages, true), 1); // Retrying.
    messages.push(ChatMessage::new(ChatRole::User, "Try again", None));
    messages.push(ChatMessage::new(ChatRole::Agent, "Recorded.", None));
    assert_eq!(board_column(0, &messages, false), 1);
}

#[test]
fn next_step_is_separate_and_no_reply_is_not_a_request() {
    assert_eq!(
        split_reply("SSO is recorded.\nYour next step: Should guests use SSO too?"),
        Reply {
            summary: "SSO is recorded.".into(),
            next: Some("Should guests use SSO too?".into()),
            no_reply: false
        }
    );
    let reply = split_reply("SSO and MFA are confirmed.\nNo reply needed.");
    assert!(reply.no_reply);
    assert_eq!(reply.summary, "SSO and MFA are confirmed.");
    assert_eq!(reply.next, None);
}
