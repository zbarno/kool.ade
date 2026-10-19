use super::super::{KooladeApp, Screen};
use crate::ui::ApplicationCommand;

mod task_detail;

impl KooladeApp {
    pub(in crate::app::root) fn dispatch_ui_command(&mut self, command: ApplicationCommand) {
        match command {
            ApplicationCommand::OpenSpecificationTrace {
                feature_uid,
                kind,
                reference,
            } => match &mut self.screen {
                Screen::Connected(project) => {
                    if let Err(error) = crate::app::root::specification_trace_navigation::open(
                        project,
                        &feature_uid,
                        kind,
                        &reference,
                    ) {
                        self.toasts.warning(error);
                    }
                }
                Screen::Welcome => self
                    .toasts
                    .warning("Connect a project before opening specification trace records."),
            },
            ApplicationCommand::CloseSpecificationTrace => {
                if let Screen::Connected(project) = &mut self.screen {
                    project.specification_trace_navigation = None;
                }
            }
            ApplicationCommand::GenerateRuntimeQaScenarios { ticket } => {
                self.generate_runtime_qa_scenarios(&ticket)
            }
            ApplicationCommand::ApproveRuntimeQaScenarios {
                ticket,
                expected_task_revision,
                proposal_sha256,
            } => {
                self.approve_runtime_qa_scenarios(&ticket, expected_task_revision, &proposal_sha256)
            }
            ApplicationCommand::ReviewRuntimeQaApplicability { ticket } => {
                self.review_runtime_qa_applicability(&ticket)
            }
            ApplicationCommand::ApproveRuntimeQaApplicability {
                ticket,
                expected_task_revision,
                proposal_sha256,
            } => self.approve_runtime_qa_applicability(
                &ticket,
                expected_task_revision,
                &proposal_sha256,
            ),
            ApplicationCommand::DecideFeatureSpecDelta {
                feature_uid,
                requirement_id,
                expected_sequence,
                adopt,
            } => match &mut self.screen {
                Screen::Connected(project) => {
                    match crate::app::root::spec_reconciliation::decide_feature_delta(
                        project,
                        &feature_uid,
                        &requirement_id,
                        expected_sequence,
                        adopt,
                    ) {
                        Ok(()) => self.toasts.success(if adopt {
                            "Feature target adopted and product specification updated."
                        } else {
                            "Feature proposal declined."
                        }),
                        Err(error) => self.toasts.warning(error),
                    }
                }
                Screen::Welcome => self
                    .toasts
                    .warning("Connect a project before reviewing feature target changes."),
            },
            ApplicationCommand::PrepareTaskChat { key } => self.prepare_task_chat(&key),
            ApplicationCommand::SendTaskReply { key } => self.submit_task_reply(&key),
            ApplicationCommand::SendImplementationDecision { key } => {
                self.submit_implementation_decision(&key)
            }
            ApplicationCommand::CancelTaskReply { key } => self.cancel_task_reply(&key),
            ApplicationCommand::RetryTaskChatSave => self.retry_task_chat_save(),
            ApplicationCommand::RetrySetupCheck => self.refresh_setup_attention(true),
            ApplicationCommand::PlanningRepository(command) => {
                self.handle_planning_repository_command(command)
            }
            ApplicationCommand::CreatePlanningTask {
                kind,
                description,
                parent_uid,
                source_branch,
                destination_branch,
                routing_overrides,
            } => {
                self.create_planning_task(
                    kind,
                    &description,
                    parent_uid,
                    source_branch,
                    destination_branch,
                    routing_overrides,
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
            ApplicationCommand::TakeOverStaleTaskClaim { ticket } => {
                self.take_over_stale_task_claim(ticket)
            }
            ApplicationCommand::ArchiveTask { ticket } => self.archive_task(&ticket),
            ApplicationCommand::CancelWork { key } => self.cancel_board_work(&key),
            ApplicationCommand::ApprovePublication { ticket } => self.approve_publication(&ticket),
            ApplicationCommand::RequestPublicationChanges { ticket } => {
                self.request_publication_changes(&ticket)
            }
            ApplicationCommand::ApproveReviewItem { id } => self.approve_review_item(&id),
            ApplicationCommand::AnswerClarification {
                uid,
                expected_revision,
                answer,
                selected_option,
            } => {
                self.answer_clarification(
                    &uid,
                    expected_revision,
                    &answer,
                    selected_option.as_deref(),
                );
            }
            ApplicationCommand::DeferClarification {
                uid,
                expected_revision,
                details,
            } => self.defer_clarification(&uid, expected_revision, details),
            ApplicationCommand::DecideOpportunity {
                uid,
                expected_revision,
                choice,
                explanation,
            } => self.decide_opportunity(&uid, expected_revision, choice, explanation),
            ApplicationCommand::RetryOpportunityPlanning {
                uid,
                expected_revision,
            } => self.retry_opportunity_planning(&uid, expected_revision),
            ApplicationCommand::SetOpportunityDiscoverySettings {
                settings,
                expected_revision,
            } => self.save_opportunity_discovery_settings(settings, expected_revision),
            ApplicationCommand::SnoozeOpportunity {
                uid,
                expected_revision,
                days,
            } => self.snooze_opportunity(&uid, expected_revision, days),
            ApplicationCommand::ApproveFeature { id } => self.approve_and_prepare_feature(&id),
            ApplicationCommand::ApproveFeatureForBoard { id } => {
                self.approve_feature_from_board(&id)
            }
            ApplicationCommand::RequestFeatureChanges { id, candidate_hash } => {
                self.request_feature_changes(&id, &candidate_hash)
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
            ApplicationCommand::TaskDetail(command) => task_detail::dispatch(self, command),
        }
    }
}
