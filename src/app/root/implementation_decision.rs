//! Save an explicit implementation decision without launching a planner turn.
use super::{ChatMessage, ChatRole, KooladeApp, Screen};

pub(super) fn latest_context(messages: &[ChatMessage], user_name: &str) -> Option<String> {
    let epoch = messages
        .iter()
        .rposition(|message| {
            message.role == ChatRole::Agent
                && (message.id.starts_with("needs-attention:")
                    || message.text.starts_with("## What Happened"))
        })
        .map_or(0, |index| index + 1);
    let messages = &messages[epoch..];
    let explicit = messages.iter().rev().find(|message| {
        message.role == ChatRole::User
            && !message.clarification
            && message.text.starts_with("I choose option (")
    });
    explicit
        .or_else(|| {
            messages.iter().rev().find(|m| {
                m.role == ChatRole::User
                    && !m.clarification
                    && !m.text.starts_with("I need clarification:")
                    && !m.text.starts_with("I'm not sure")
                    && !m.text.trim_end().ends_with('?')
                    && m.text
                        != "I approve creating a pull request for the verified implementation."
                    && m.text != "I am requesting changes before approving a pull request."
            })
        })
        .map(|message| format_answer(message, user_name))
}

fn format_answer(message: &ChatMessage, user_name: &str) -> String {
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
}

pub(super) fn latest_context_for(
    messages: &[ChatMessage],
    user_name: &str,
    context: Option<&str>,
) -> Option<String> {
    if let Some(context) = context {
        messages
            .iter()
            .rev()
            .find(|message| {
                message.role == ChatRole::User
                    && !message.clarification
                    && message.answer_context.as_deref() == Some(context)
            })
            .map(|message| format_answer(message, user_name))
            .or_else(|| {
                let epoch = messages.iter().rposition(|message| {
                    message.role == ChatRole::Agent && message.id.starts_with("needs-attention:")
                })?;
                if messages[epoch].id != context {
                    return None;
                }
                let legacy = messages[epoch + 1..]
                    .iter()
                    .filter(|message| message.answer_context.is_none())
                    .cloned()
                    .collect::<Vec<_>>();
                latest_context(&legacy, user_name)
            })
    } else {
        latest_context(messages, user_name)
    }
}

impl KooladeApp {
    pub(super) fn submit_implementation_decision(&mut self, key: &str) -> bool {
        let Screen::Connected(project) = &mut self.screen else {
            return false;
        };
        let blocked = project.queue.blocked.contains_key(key)
            || project.implementation_states.get(key).is_some_and(|state| {
                state.status == crate::core::implementation::ImplementationStatus::Blocked
            });
        if project.task_turns.contains_key(key) || !blocked {
            return false;
        }
        let text = project
            .task_chats
            .drafts
            .get(key)
            .cloned()
            .unwrap_or_default();
        if text.trim().is_empty() {
            return false;
        }
        project.bind_task_conversation_identities();
        project.task_chats.ensure_loaded(&project.chat_slug);
        let mut message = ChatMessage::new(ChatRole::User, &text, Some(key.into()));
        let detail = project
            .queue
            .blocked
            .get(key)
            .map(|failure| failure.message.as_str())
            .or_else(|| {
                project
                    .implementation_states
                    .get(key)
                    .map(|state| state.detail.as_str())
            });
        message.answer_context = detail.map(|detail| {
            crate::core::attention::answer_context(&project.state.repo_root, key, detail)
        });
        if let Err(error) = project
            .task_chats
            .append(&project.chat_slug, key, vec![message])
        {
            self.toasts.warning(error);
            return false;
        }
        project.task_chats.drafts.remove(key);
        project.activity.pending.push(format!(
            "User supplied an implementation decision for {key}: {}",
            crate::core::context_build::clip(&text, 1200)
        ));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_typed_free_text_answer_supersedes_an_older_option() {
        let mut option = ChatMessage::new(ChatRole::User, "I choose option (old): Old plan.", None);
        option.answer_context = Some("current".into());
        let mut correction =
            ChatMessage::new(ChatRole::User, "My answer: Use the label Why?", None);
        correction.answer_context = Some("current".into());
        let context = latest_context_for(&[option, correction], "Morgan", Some("current")).unwrap();
        assert!(context.contains("Use the label Why?"));
        assert!(!context.contains("Old plan"));
    }

    #[test]
    fn legacy_answers_are_reused_only_for_their_matching_attention_event() {
        let mut event =
            ChatMessage::new(ChatRole::Agent, "A saved question needs an answer.", None);
        event.id = "needs-attention:legacy".into();
        let answer = ChatMessage::new(
            ChatRole::User,
            "I choose option (keep): Keep the plan.",
            None,
        );
        let mut messages = vec![event, answer];
        assert!(latest_context_for(&messages, "Morgan", Some("needs-attention:legacy")).is_some());
        assert!(latest_context_for(&messages, "Morgan", Some("needs-attention:other")).is_none());
        let mut question =
            ChatMessage::new(ChatRole::User, "Please explain the consequences", None);
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
}
