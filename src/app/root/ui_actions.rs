use super::{PacketApp, Screen, persist_automation_settings};
use crate::{
    domain::{ChatMessage, ChatRole},
    ui::{ApplicationCommand, task_detail::Command as TaskDetailCommand},
};

mod intent;

impl PacketApp {
    pub(super) fn dispatch_ui_command(&mut self, command: ApplicationCommand) {
        match command {
            ApplicationCommand::PrepareTaskChat { key } => self.prepare_task_chat(&key),
            ApplicationCommand::SendTaskReply { key } => self.submit_task_reply(&key),
            ApplicationCommand::SendImplementationDecision { key } => {
                self.submit_implementation_decision(&key)
            }
            ApplicationCommand::CancelTaskReply { key } => self.cancel_task_reply(&key),
            ApplicationCommand::RetryTaskChatSave => self.retry_task_chat_save(),
            ApplicationCommand::DrainTaskChatSaves => self.drain_task_chat_saves(),
            ApplicationCommand::CancelTask => self.cancel_task(),
            ApplicationCommand::SetMaxParallelTasks { count } => self.set_max_parallel_tasks(count),
            ApplicationCommand::SetAutoPlan { enabled } => self.set_auto_plan(enabled),
            ApplicationCommand::SetAutoBuild { enabled } => self.set_auto_build(enabled),
            ApplicationCommand::SetAutoPublish { enabled } => self.set_auto_publish(enabled),
            ApplicationCommand::SetRequireIndependentChecks { enabled } => {
                self.set_require_independent_checks(enabled)
            }
            ApplicationCommand::ArchiveTask { ticket } => self.archive_task(&ticket),
            ApplicationCommand::ApproveReviewItem { id } => self.approve_review_item(&id),
            ApplicationCommand::ApproveFeature { id } => self.approve_and_prepare_feature(&id),
            ApplicationCommand::UserIntent(intent) => self.apply_user_intent(intent),
            ApplicationCommand::HeaderAction(action) => self.apply_header_action(action),
            ApplicationCommand::TaskDetail(command) => self.dispatch_task_detail(command),
        }
    }

    pub(crate) fn prepare_task_chat(&mut self, key: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project
            .task_chats
            .messages
            .get(key)
            .is_some_and(|messages| !messages.is_empty())
        {
            return;
        }
        project.bind_task_conversation_identities();
        project.task_chats.ensure_loaded(&project.chat_slug);
        if project
            .task_chats
            .messages
            .get(key)
            .is_some_and(|messages| !messages.is_empty())
        {
            return;
        }
        let Some((_, greeting)) = crate::core::task_conversation::presentation(
            &project.state,
            &project.task_documents,
            key,
        ) else {
            return;
        };
        let mut message = ChatMessage::new(ChatRole::Agent, greeting, Some(key.into()));
        message.id = format!("task-introduction:{key}");
        project
            .task_chats
            .remember_response(&project.chat_slug, key, vec![message]);
    }

    pub(crate) fn cancel_task_reply(&mut self, key: &str) {
        if let Screen::Connected(project) = &self.screen
            && let Some(turn) = project.task_turns.get(key)
        {
            turn.request_cancel();
        }
    }

    fn retry_task_chat_save(&mut self) {
        if let Screen::Connected(project) = &mut self.screen {
            project.task_chats.retry_save(&project.chat_slug);
        }
    }

    fn drain_task_chat_saves(&mut self) {
        if let Screen::Connected(project) = &mut self.screen {
            project.task_chats.drain_if_pending(&project.chat_slug);
        }
    }

    pub(super) fn cancel_task(&mut self) {
        if let Screen::Connected(project) = &mut self.screen {
            project.queue.running = false;
            project.queue.recovery_paused = true;
            if project.queue_lock.is_some()
                && let Err(error) = project.queue.save(&project.state.repo_root)
            {
                project.queue.last_error = error.to_string();
            }
            for controller in project.active_implementations.values() {
                controller.request_cancel();
            }
        }
    }

    fn set_max_parallel_tasks(&mut self, count: usize) {
        if let Screen::Connected(project) = &mut self.screen {
            let temporary_lock = if project.queue_lock.is_none() {
                match crate::core::implementation_queue::Queue::acquire(&project.state.repo_root) {
                    Ok(lock) => Some(lock),
                    Err(error) => {
                        project.queue.last_error = error.to_string();
                        return;
                    }
                }
            } else {
                None
            };
            let previous = project.queue.max_parallel;
            project.queue.max_parallel = count.clamp(1, 8);
            if let Err(error) = project.queue.save(&project.state.repo_root) {
                project.queue.max_parallel = previous;
                project.queue.last_error = error.to_string();
            }
            drop(temporary_lock);
        }
    }

    pub(super) fn dispatch_task_detail(&mut self, command: TaskDetailCommand) {
        match command {
            TaskDetailCommand::UpdateDraft { ticket, draft } => {
                if let Screen::Connected(project) = &mut self.screen {
                    project.task_chats.drafts.insert(ticket, draft);
                }
            }
            TaskDetailCommand::SubmitReply {
                ticket,
                draft,
                decision,
            } => {
                if let Screen::Connected(project) = &mut self.screen {
                    project.task_chats.drafts.insert(ticket.clone(), draft);
                }
                if decision {
                    self.submit_implementation_decision(&ticket);
                } else {
                    self.submit_task_reply(&ticket);
                }
            }
            TaskDetailCommand::RetryChatSave => self.retry_task_chat_save(),
            TaskDetailCommand::RetryExplanation { ticket, detail } => {
                self.retry_attention(&ticket, &detail)
            }
            TaskDetailCommand::StopAndPause { ticket } => {
                if let Screen::Connected(project) = &mut self.screen {
                    project.queue.running = false;
                    project.queue.recovery_paused = true;
                    if let Some(controller) = project.active_implementations.get(&ticket) {
                        controller.request_cancel();
                    }
                    if project.queue_lock.is_some()
                        && let Err(error) = project.queue.save(&project.state.repo_root)
                    {
                        project.queue.last_error = error.to_string();
                    }
                }
            }
            TaskDetailCommand::StartOrResume { ticket } => self.start_implementation(ticket, true),
        }
    }

    fn set_auto_plan(&mut self, enabled: bool) {
        if let Screen::Connected(project) = &mut self.screen {
            project.queue.auto_plan = enabled;
            if !enabled {
                project.activity.manager = None;
                if let Some(investigation) = &project.investigation {
                    investigation.cancel();
                }
            }
            if let Err(error) = persist_automation_settings(project) {
                project.queue.last_error = error;
            }
        }
    }

    fn set_auto_build(&mut self, enabled: bool) {
        if let Screen::Connected(project) = &mut self.screen {
            project.queue.auto_build = enabled;
            if !enabled {
                project.queue.running = false;
            }
            if let Err(error) = persist_automation_settings(project) {
                project.queue.last_error = error;
            }
        }
    }

    fn set_auto_publish(&mut self, enabled: bool) {
        if let Screen::Connected(project) = &mut self.screen {
            project.queue.auto_publish = enabled;
            if enabled {
                project.queue.require_independent_checks = true;
            }
            if !enabled {
                for controller in project.active_implementations.values() {
                    controller.disable_automatic_publication();
                }
            }
            if let Err(error) = persist_automation_settings(project) {
                project.queue.last_error = error;
            }
        }
    }

    fn set_require_independent_checks(&mut self, enabled: bool) {
        if let Screen::Connected(project) = &mut self.screen {
            project.queue.require_independent_checks = enabled || project.queue.auto_publish;
            if let Err(error) = persist_automation_settings(project) {
                project.queue.last_error = error;
            }
        }
    }

    fn archive_task(&mut self, ticket: &str) {
        let done = matches!(&self.screen, Screen::Connected(project)
            if crate::core::planning_work::cards(&project.state, &project.planning_work)
                .iter().any(|work| work.key == ticket && work.column == 4))
            || match &self.screen {
                Screen::Connected(project) => {
                    project
                        .state
                        .resolved_items
                        .iter()
                        .any(|item| item.conversation_key() == ticket)
                        || project
                            .implementation_states
                            .get(ticket)
                            .is_some_and(|state| {
                                state.status == super::ImplementationStatus::Completed
                            })
                }
                _ => false,
            };
        let result = {
            let Screen::Connected(project) = &mut self.screen else {
                return;
            };
            if project.active_implementations.contains_key(ticket)
                || project.task_turns.contains_key(ticket)
                || !done
            {
                return;
            }
            let mut archived = project.archived_tasks.clone();
            archived.insert(ticket.to_string());
            crate::persistence::archived_tasks::save(&project.chat_slug, &archived)
                .inspect(|()| project.archived_tasks = archived)
        };
        if let Err(error) = result {
            self.toasts
                .danger(format!("Could not archive task: {error}"));
        }
    }

    fn approve_review_item(&mut self, id: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some() {
            self.toasts
                .warning("Finish the active planning turn before approving this review.");
            return;
        }
        match crate::core::board_actions::approve_review(&mut project.state, id) {
            Ok(_) => {
                project.next_question_id = None;
                project.activity.pending.push(format!(
                    "Approved review {id}; the feature decision and board are updated."
                ));
                self.toasts.success(format!("Approved review {id}"));
            }
            Err(error) => {
                if let Ok(current) =
                    crate::core::state::PlannerState::load(&project.state.repo_root)
                {
                    project.state = current;
                }
                self.toasts
                    .danger(format!("Could not approve {id}: {error}"));
            }
        }
    }
}
