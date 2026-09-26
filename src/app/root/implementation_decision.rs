//! Save an explicit implementation decision without launching a planner turn.
use super::{ChatMessage, ChatRole, PacketApp, Screen};

pub(super) fn latest_context(messages: &[ChatMessage], user_name: &str) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|m| m.role == ChatRole::User)
        .map(|message| {
            format!(
                "Submitted at {} by {}:\n{}",
                message.ts,
                if user_name.trim().is_empty() {
                    "the current user"
                } else {
                    user_name
                },
                message.text
            )
        })
}

impl PacketApp {
    pub(super) fn submit_implementation_decision(&mut self, key: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let blocked = project.queue.blocked.contains_key(key)
            || project.implementation_states.get(key).is_some_and(|state| {
                state.status == crate::core::implementation::ImplementationStatus::Blocked
            });
        if project.task_turns.contains_key(key) || !blocked {
            return;
        }
        let text = project
            .task_chats
            .drafts
            .get(key)
            .cloned()
            .unwrap_or_default();
        if text.trim().is_empty() {
            return;
        }
        project.bind_task_conversation_identities();
        project.task_chats.ensure_loaded(&project.chat_slug);
        let message = ChatMessage::new(ChatRole::User, &text, Some(key.into()));
        if let Err(error) = project
            .task_chats
            .append(&project.chat_slug, key, vec![message])
        {
            self.toasts.warning(error);
            return;
        }
        project.task_chats.drafts.remove(key);
        project.activity.pending.push(format!(
            "User supplied an implementation decision for {key}: {}",
            crate::core::context_build::clip(&text, 1200)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
