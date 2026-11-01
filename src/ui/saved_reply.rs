//! Present durable replies independently from the card's unresolved status.
use crate::domain::{ChatMessage, ChatRole};

pub(crate) fn label(messages: &[ChatMessage]) -> Option<&'static str> {
    let user = messages
        .iter()
        .rposition(|message| message.role == ChatRole::User && !message.clarification)?;
    let after = &messages[user + 1..];
    if after
        .iter()
        .any(|message| message.role == ChatRole::Agent && !message.clarification)
        || after.iter().any(|message| {
            message
                .id
                .starts_with("internal-task-reply-outcome:applied:")
                || message
                    .id
                    .starts_with("internal-retry:planning-write-applied:")
        })
    {
        return Some("Answer recorded; awaiting remaining work");
    }
    if after
        .iter()
        .rev()
        .find(|message| message.role == ChatRole::System)
        .is_some_and(|message| {
            message.id.starts_with("internal-task-reply-outcome:")
                || message.id.starts_with("internal-task-reply-recoverable:")
        })
    {
        Some("Answer saved; internal recovery needs attention")
    } else {
        Some("Answer saved; processing internally")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_processing_failure_and_application_have_distinct_labels() {
        assert_eq!(label(&[]), None);
        let mut messages = vec![ChatMessage::new(ChatRole::User, "My answer", None)];
        assert_eq!(
            label(&messages),
            Some("Answer saved; processing internally")
        );
        let mut failure = ChatMessage::new(ChatRole::System, "Provider unavailable", None);
        crate::persistence::task_chats::TaskChats::mark_task_reply_outcome(&mut failure);
        messages.push(failure);
        assert_eq!(
            label(&messages),
            Some("Answer saved; internal recovery needs attention")
        );
        messages.push(ChatMessage::new(
            ChatRole::Agent,
            "Choice applied; review remains",
            None,
        ));
        assert_eq!(
            label(&messages),
            Some("Answer recorded; awaiting remaining work")
        );
    }
}
