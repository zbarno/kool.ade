use super::super::{KooladeApp, Screen};
use crate::ui::{ApplicationCommand, task_detail::Command as TaskDetailCommand};

impl KooladeApp {
    pub(in crate::app::root) fn dispatch_ui_command(&mut self, command: ApplicationCommand) {
        match command {
            ApplicationCommand::PrepareTaskChat { key } => self.prepare_task_chat(&key),
            ApplicationCommand::SendTaskReply { key } => self.submit_task_reply(&key),
            ApplicationCommand::SendImplementationDecision { key } => {
                self.submit_implementation_decision(&key)
            }
            ApplicationCommand::CancelTaskReply { key } => self.cancel_task_reply(&key),
            ApplicationCommand::RetryTaskChatSave => self.retry_task_chat_save(),
            ApplicationCommand::RetrySetupCheck => self.refresh_setup_attention(true),
            ApplicationCommand::CreatePlanningTask {
                kind,
                description,
                parent_uid,
                source_branch,
                destination_branch,
            } => {
                self.create_planning_task(
                    kind,
                    &description,
                    parent_uid,
                    source_branch,
                    destination_branch,
                );
            }
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
            ApplicationCommand::CancelWork { key } => self.cancel_board_work(&key),
            ApplicationCommand::ApprovePublication { ticket } => self.approve_publication(&ticket),
            ApplicationCommand::RequestPublicationChanges { ticket } => {
                self.request_publication_changes(&ticket)
            }
            ApplicationCommand::ApproveReviewItem { id } => self.approve_review_item(&id),
            ApplicationCommand::ApproveFeature { id } => self.approve_and_prepare_feature(&id),
            ApplicationCommand::ApproveFeatureForBoard { id } => {
                self.approve_feature_from_board(&id)
            }
            ApplicationCommand::GenerateTasksForFeature {
                work_key,
                feature_id,
            } => self.start_task_generation(&work_key, &feature_id),
            ApplicationCommand::CompareFeaturePlans { id } => self.start_comparison_turn(&id),
            ApplicationCommand::ChooseFeaturePlan { id, plan_id } => {
                self.choose_feature_plan(&id, &plan_id)
            }
            ApplicationCommand::DiscardFeaturePlans { id } => self.discard_feature_plans(&id),
            ApplicationCommand::UserIntent(intent) => self.apply_user_intent(intent),
            ApplicationCommand::HeaderAction(action) => self.apply_header_action(action),
            ApplicationCommand::TaskDetail(command) => match command {
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
                TaskDetailCommand::StopAndPause { ticket } => self.stop_and_pause_task(&ticket),
                TaskDetailCommand::StartOrResume { ticket } => {
                    self.start_implementation(ticket, true)
                }
            },
        }
    }

    fn stop_and_pause_task(&mut self, ticket: &str) {
        if let Screen::Connected(project) = &mut self.screen {
            project.queue.running = false;
            project.queue.recovery_paused = true;
            if let Some(controller) = project.active_implementations.get(ticket) {
                controller.request_cancel();
            }
            if project.queue_lock.is_some()
                && let Err(error) = project.queue.save(&project.state.repo_root)
            {
                project.queue.last_error = error.to_string();
            }
        }
    }
}
