use super::*;

impl KooladeApp {
    pub(in crate::app::root) fn approve_publication(&mut self, ticket: &str) {
        if self.task_identity_is_ambiguous(ticket) {
            return;
        }
        let eligible = matches!(&self.screen, Screen::Connected(project)
            if !project.active_implementations.contains_key(ticket)
                && project.implementation_states.get(ticket).is_some_and(|state|
                    state.pr_url.is_none()
                        && matches!(state.status,
                            ImplementationStatus::AwaitingApproval
                                | ImplementationStatus::ReadyToPublish)));
        if eligible {
            if let Screen::Connected(project) = &mut self.screen {
                project.task_chats.ensure_loaded(&project.chat_slug);
                const DECISION: &str =
                    "I approve creating a pull request for the verified implementation.";
                if !project
                    .task_chats
                    .messages
                    .get(ticket)
                    .is_some_and(|messages| messages.iter().any(|message| message.text == DECISION))
                    && let Err(error) = project.task_chats.append(
                        &project.chat_slug,
                        ticket,
                        vec![ChatMessage::new(
                            ChatRole::User,
                            DECISION,
                            Some(ticket.to_owned()),
                        )],
                    )
                {
                    self.toasts.warning(error);
                    return;
                }
            }
            self.start_implementation(ticket.to_owned(), true);
        }
    }

    pub(in crate::app::root) fn request_publication_changes(&mut self, ticket: &str) {
        if self.task_identity_is_ambiguous(ticket) {
            return;
        }
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_implementations.contains_key(ticket) {
            return;
        }
        let Some(mut state) = project.implementation_states.get(ticket).cloned() else {
            return;
        };
        if !matches!(
            state.status,
            ImplementationStatus::AwaitingApproval | ImplementationStatus::ReadyToPublish
        ) || state.pr_url.is_some()
        {
            return;
        }
        let Ok(dir) = crate::core::implementation::state_dir_for_task(
            &project.state.repo_root,
            ticket,
            state.task_uid.as_deref(),
        ) else {
            return;
        };
        project.task_chats.ensure_loaded(&project.chat_slug);
        if let Err(error) = project.task_chats.append(
            &project.chat_slug,
            ticket,
            vec![crate::domain::ChatMessage::new(
                crate::domain::ChatRole::User,
                "I am requesting changes before approving a pull request.",
                Some(ticket.to_owned()),
            )],
        ) {
            self.toasts.warning(error);
            return;
        }
        state.status = ImplementationStatus::ChangesRequested;
        state.detail = "Changes requested before PR approval. Describe the requested changes in this task's conversation to resume implementation.".into();
        if let Err(error) = crate::core::implementation::save(&dir, &state) {
            project.queue.last_error = format!("Cannot save the review decision: {error:#}");
            self.toasts.warning(
                "The change request was saved in the task conversation, but its implementation status could not be updated.",
            );
            return;
        }
        project
            .implementation_states
            .insert(ticket.to_owned(), state);
    }

    fn task_identity_is_ambiguous(&mut self, ticket: &str) -> bool {
        let ambiguous = match &mut self.screen {
            Screen::Connected(project) => project.task_conversation_identity_is_ambiguous(ticket),
            Screen::Welcome => false,
        };
        if ambiguous {
            self.toasts.warning(
                "This board conversation has conflicting artifact identities. The review action is blocked until Kool.ad/e repairs the board link.",
            );
        }
        ambiguous
    }
}
