use super::*;
use chat::remember_turn_messages;

impl KooladeApp {
    pub(super) fn adopt_rejected_turn(
        &mut self,
        project: &mut Project,
        problems: Vec<String>,
        _final_text: String,
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
                format!(
                    "The saved answer could not be applied because its feature identity could not be verified. Manager owns internal triage; your answer remains saved. Diagnostic: {}",
                    problems.join("; ")
                ),
                None,
            );
            if conversation.is_some() || project.task_chats.active.is_some() {
                crate::persistence::task_chats::TaskChats::mark_task_reply_outcome(&mut paused);
            }
            remember_turn_messages(project, conversation, vec![paused]);
            self.toasts
                .warning("Manager is triaging the feature identity failure. Your answer is saved.");
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
            if (conversation.is_some() || project.task_chats.active.is_some())
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
        if (conversation.is_some() || project.task_chats.active.is_some())
            && let Some(message) = chat.first_mut()
        {
            if !problems.is_empty()
                && problems.iter().all(|problem| {
                    problem.starts_with("Structured JSON block is malformed")
                        || problem.contains("did not include the required structured JSON block")
                        || problem.contains("cannot update and resolve the same item in one turn")
                })
            {
                crate::persistence::task_chats::TaskChats::mark_recoverable_task_reply(message);
            } else {
                crate::persistence::task_chats::TaskChats::mark_task_reply_outcome(message);
            }
        }
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
                _ => format!(
                    "Planning stopped: {}",
                    crate::error::redact_secrets(&error.detail())
                ),
            },
            None,
        );
        if conversation.is_some() || project.task_chats.active.is_some() {
            if matches!(&error, crate::error::AppError::HarnessExecutionFailed { failure } if failure.retryable)
            {
                crate::persistence::task_chats::TaskChats::mark_recoverable_task_reply(
                    &mut stopped,
                );
            } else {
                crate::persistence::task_chats::TaskChats::mark_task_reply_outcome(&mut stopped);
            }
        }
        remember_turn_messages(project, conversation, vec![stopped]);
        self.toasts.danger(error.headline());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Production boundary: a background task's rejected result is persisted as
    /// failed even with its modal closed, so Manager can create recovery work.
    /// Direct marker tests cannot prove adoption selects the correct conversation.
    /// Temporary local fixture storage; no model/Docker/network, low setup cost.
    #[test]
    fn background_rejection_is_owned_without_an_open_task_modal() {
        let mut app = crate::app::root::board_tests::fixture();
        let Screen::Connected(mut project) = std::mem::replace(&mut app.screen, Screen::Welcome)
        else {
            panic!("connected fixture");
        };
        let mut work = crate::core::planning_work::Work::new(
            String::new(),
            "Keep original intent".into(),
            "Synthetic planning request".into(),
            String::new(),
        );
        let key = format!("task:{}", work.uid);
        work.key = key.clone();
        project.planning_work.push(work);
        project.save_planning_work().unwrap();
        project.bind_task_conversation_identities();
        let slug = project.chat_slug.clone();
        project.task_chats.ensure_loaded(&slug);
        project
            .task_chats
            .append_user_reply_and_claim(
                &slug,
                &key,
                ChatMessage::new(ChatRole::User, "Keep the saved choice", Some(key.clone())),
            )
            .unwrap();
        project.task_chats.active = None;
        app.adopt_rejected_turn(
            &mut project,
            vec![
                "Planning task feature identity mismatch: the recorded feature UID is unavailable"
                    .into(),
            ],
            String::new(),
            Some(&key),
        );
        assert!(project.task_chats.has_failed_task_reply(&key));
        crate::app::manager::operational_recovery::advance(&mut project);
        assert_eq!(
            project
                .planning_work
                .iter()
                .filter(|work| work.operational_recovery.is_some())
                .count(),
            1
        );
        assert!(
            project.task_chats.messages[&key]
                .iter()
                .any(|message| message.text.contains("recorded feature UID is unavailable"))
        );
    }
}
