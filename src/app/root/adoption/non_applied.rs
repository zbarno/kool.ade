use super::*;
use chat::remember_turn_messages;

impl KooladeApp {
    pub(super) fn adopt_rejected_turn(
        &mut self,
        project: &mut Project,
        problems: Vec<String>,
        final_text: String,
        conversation: Option<&str>,
    ) {
        let concurrent_write_conflict = problems
            .iter()
            .any(|problem| problem.starts_with("Planning files changed on disk"));
        let stale_feature_identity = problems
            .iter()
            .any(|problem| problem.starts_with("Planning task feature identity mismatch:"));
        if stale_feature_identity {
            let mut paused = ChatMessage::new(
                ChatRole::System,
                "This task was paused because its linked feature was replaced or removed. Kool.ad/e will not retarget the old conversation by display ID. To continue against the current feature, create a new planning task from that feature; this conversation remains available for reference.",
                None,
            );
            if project.task_chats.active.is_some() {
                crate::persistence::task_chats::TaskChats::mark_task_reply_outcome(&mut paused);
            }
            remember_turn_messages(project, conversation, vec![paused]);
            self.toasts.warning(
                "This task's linked feature was replaced or removed. Create a new task from the current feature to continue.",
            );
            return;
        }
        let mut chat = vec![ChatMessage::new(
            ChatRole::System,
            format!(
                "⚠ Turn rejected — nothing was written.\n{}",
                problems.join("\n")
            ),
            None,
        )];
        if concurrent_write_conflict {
            if conversation.is_some()
                && let Some(message) = chat.first_mut()
            {
                crate::persistence::task_chats::TaskChats::mark_planning_conflict_retry(message);
            }
            remember_turn_messages(project, conversation, chat);
            self.toasts.danger(
                "Planning change was not applied; the submitted message is preserved for an internal retry.",
            );
            return;
        }
        if project.task_chats.active.is_some()
            && let Some(message) = chat.first_mut()
        {
            crate::persistence::task_chats::TaskChats::mark_task_reply_outcome(message);
        }
        chat.push(ChatMessage::new(
            ChatRole::Agent,
            if final_text.trim().is_empty() {
                "(no legible reply — try again)".to_string()
            } else {
                final_text
            },
            None,
        ));
        remember_turn_messages(project, conversation, chat);
        self.toasts.danger(format!(
            "Rejected: {}",
            problems.first().map(String::as_str).unwrap_or("")
        ));
    }

    pub(super) fn adopt_failed_turn(
        &mut self,
        project: &mut Project,
        error: crate::error::AppError,
        conversation: Option<&str>,
    ) {
        let mut stopped = ChatMessage::new(
            ChatRole::System,
            match &error {
                crate::error::AppError::InvalidResponse { .. } => {
                    format!("Task generation needs attention: {}", error.detail())
                }
                _ => format!("Planning stopped: {}", error.headline()),
            },
            None,
        );
        if project.task_chats.active.is_some() {
            crate::persistence::task_chats::TaskChats::mark_task_reply_outcome(&mut stopped);
        }
        remember_turn_messages(project, conversation, vec![stopped]);
        self.toasts.danger(error.headline());
    }
}
