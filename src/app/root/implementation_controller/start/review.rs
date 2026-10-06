use super::*;

impl KooladeApp {
    pub(in crate::app::root) fn approve_publication(&mut self, ticket: &str) {
        let eligible = matches!(&self.screen, Screen::Connected(project)
            if !project.active_implementations.contains_key(ticket)
                && project.implementation_states.get(ticket).is_some_and(|state|
                    state.pr_url.is_none()
                        && matches!(state.status,
                            ImplementationStatus::AwaitingApproval
                                | ImplementationStatus::ReadyToPublish)));
        if eligible {
            if let Screen::Connected(project) = &mut self.screen {
                project.bind_task_conversation_identities();
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
        state.status = ImplementationStatus::ChangesRequested;
        state.detail = "Changes requested before PR approval. Describe the requested changes in this task's conversation to resume implementation.".into();
        let Ok(dir) = crate::core::implementation::state_dir_for_task(
            &project.state.repo_root,
            ticket,
            state.task_uid.as_deref(),
        ) else {
            return;
        };
        if let Err(error) = crate::core::implementation::save(&dir, &state) {
            project.queue.last_error = format!("Cannot save the review decision: {error:#}");
            return;
        }
        project
            .implementation_states
            .insert(ticket.to_owned(), state);
        project.bind_task_conversation_identities();
        project.task_chats.ensure_loaded(&project.chat_slug);
        project.task_chats.remember_response(
            &project.chat_slug,
            ticket,
            vec![crate::domain::ChatMessage::new(
                crate::domain::ChatRole::User,
                "I am requesting changes before approving a pull request.",
                Some(ticket.to_owned()),
            )],
        );
    }
}
