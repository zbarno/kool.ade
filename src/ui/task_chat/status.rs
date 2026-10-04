use crate::domain::{ChatMessage, ChatRole};
#[derive(Default, Debug, PartialEq)]
pub(crate) struct Reply {
    pub(crate) summary: String,
    pub(crate) next: Option<String>,
    pub(crate) no_reply: bool,
}

pub(crate) fn split_reply(text: &str) -> Reply {
    // Delegated to the shared tail classifier; the mapping onto the legacy
    // struct is the byte-for-byte compatibility contract (asserted in tests).
    let tail = crate::ui::reply_tail::parse_reply_tail(text);
    Reply {
        summary: tail.body,
        next: tail.ask,
        no_reply: tail.no_reply,
    }
}

pub(super) fn failed(messages: &[ChatMessage]) -> bool {
    messages
        .iter()
        .rev()
        .take_while(|m| m.role != ChatRole::User)
        .any(|m| {
            m.role == ChatRole::System
                && (m.text.starts_with("⚠ Turn rejected")
                    || m.text.starts_with("Planning stopped:")
                    || m.text.starts_with("Task generation needs attention:"))
        })
}

/// Conversation participation is durable progress, not implementation completion.
/// Explicit review, blocker and terminal states take precedence over discussion.
pub(crate) fn board_column(base: usize, messages: &[ChatMessage], active: bool) -> usize {
    if base >= 2 {
        return base;
    }
    if active {
        return 1;
    }
    if failed(messages) || messages.last().is_some_and(|m| m.role == ChatRole::User) {
        return 3;
    }
    if messages.iter().any(|m| m.role == ChatRole::User) {
        return 1;
    }
    base
}
