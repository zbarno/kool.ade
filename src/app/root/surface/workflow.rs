macro_rules! workflow_surface_methods {
    () => {
        fn active_planning_work(&self) -> Option<&str> {
            match &self.screen {
                Screen::Connected(project) if project.active_turn.is_some() => {
                    project.active_planning_work.as_deref()
                }
                _ => None,
            }
        }

        fn task_offer(&self) -> Option<&crate::core::workflow::InterviewBrief> {
            match &self.screen {
                Screen::Connected(p)
                    if p.active_turn.is_none()
                        && p.active_implementations.is_empty()
                        && !has_current_task_batch(p)
                        && p.state.workflow.ready(p.state.planning_contract()) =>
                {
                    p.state.workflow.brief.as_ref()
                }
                _ => None,
            }
        }

        fn implementation_offer(&self) -> bool {
            matches!(&self.screen, Screen::Connected(p)
                if p.active_turn.is_none()
                    && p.active_implementations.is_empty()
                    && has_current_task_batch(p)
                    && matches!(crate::core::implementation_queue::next_ready_ticket_with_lifecycle_subtasks(
                        &p.task_documents,
                        &p.implementation_states,
                        &Default::default(),
                        &Default::default(),
                        &crate::app::root::implementation_controller::completed_subtask_uids(p),
                    ), Ok(Some(_))))
        }

        fn dispatch(&mut self, command: crate::ui::ApplicationCommand) {
            self.dispatch_ui_command(command);
        }

        fn planning_board(&self) -> crate::ui::planning_board::ViewModel {
            self.planning_board_for_surface()
        }

        fn next_question_id(&self) -> Option<&str> {
            match &self.screen {
                Screen::Connected(p) => p.next_question_id.as_deref(),
                Screen::Welcome => None,
            }
        }

        fn spec_text(&self) -> &str {
            match &self.screen {
                Screen::Connected(p) => p
                    .live_progress
                    .specification
                    .as_deref()
                    .or(p.state.spec_text.as_deref())
                    .unwrap_or(NO_SPEC_PLACEHOLDER),
                Screen::Welcome => "",
            }
        }

        fn active_features(&self) -> Vec<(&str, &str)> {
            match &self.screen {
                Screen::Connected(p) => p
                    .state
                    .active_features
                    .iter()
                    .map(|(id, body)| (id.as_str(), body.as_str()))
                    .collect(),
                Screen::Welcome => Vec::new(),
            }
        }

        fn feature_approved(&self, id: &str) -> bool {
            match &self.screen {
                Screen::Connected(p) => crate::core::workflow::feature_approved(
                    &p.state.planning_store,
                    &p.state.workflow,
                    id,
                ),
                Screen::Welcome => false,
            }
        }

        fn feature_actions(
            &self,
            conversation: Option<&str>,
        ) -> Vec<crate::ui::feature_approval::Action> {
            self.available_feature_actions(conversation)
        }

        fn toasts(&mut self) -> &mut ToastQueue {
            &mut self.toasts
        }
    };
}

pub(super) use workflow_surface_methods;
