use super::super::{Dialog, KooladeApp, Screen};
use crate::ui::{HeaderAction, Intent, Surface};

impl KooladeApp {
    pub(super) fn apply_user_intent(&mut self, intent: Intent) {
        if let Some((id, candidate_hash)) = &intent.request_feature_changes {
            self.request_feature_changes(id, candidate_hash);
            return;
        }
        if let Some(id) = &intent.approve_feature {
            if let Screen::Connected(project) = &mut self.screen {
                project.remember_chat(vec![crate::domain::ChatMessage::new(
                    crate::domain::ChatRole::System,
                    format!("Open {id}'s review card on the Kanban to read and approve its current specification."),
                    None,
                )]);
            }
            return;
        }
        if intent.implement_tasks {
            if self.implementation_offer() {
                super::super::requested_action::start_next(self);
            }
            return;
        }
        if intent.generate_tasks {
            if let Screen::Connected(project) = &mut self.screen {
                project.remember_chat(vec![crate::domain::ChatMessage::new(
                    crate::domain::ChatRole::System,
                    "Start task generation from the approved feature's Generate tasks card on the Kanban.",
                    None,
                )]);
            }
            return;
        }
        if intent.cancel {
            self.pending_feature_generation = None;
            if let Screen::Connected(project) = &mut self.screen
                && let Some(controller) = &project.active_turn
            {
                controller.request_cancel();
                self.toasts.warning("Cancellation requested…");
            }
            return;
        }
        if intent.send {
            let taken = match &mut self.screen {
                Screen::Connected(project) => std::mem::take(&mut project.draft),
                Screen::Welcome => return,
            };
            let text = taken.trim();
            if !text.is_empty() {
                self.start_turn(text);
            }
        }
    }

    pub(super) fn apply_header_action(&mut self, action: HeaderAction) {
        match action {
            HeaderAction::Refresh => {
                if let Screen::Connected(project) = &mut self.screen {
                    project.last_pr_refresh = None;
                    project.refresh_git();
                    project.refresh_implementations();
                    project.task_documents = crate::artifacts::task_docs::load_board(
                        &project.state.planning_store,
                        &project.state.workflow,
                    );
                    self.toasts.info("Git state refreshed");
                }
            }
            HeaderAction::Import => {
                self.dialog = Some(Dialog::Import(super::super::DlgImport::new()))
            }
            HeaderAction::McpServers => {
                if let Screen::Connected(project) = &self.screen {
                    self.dialog = Some(Dialog::Mcp(super::super::DlgMcp::from_project(project)));
                }
            }
            HeaderAction::CopySpec => self.copy_spec_to_clipboard(),
            HeaderAction::OpenWorkspace => self.open_workspace(),
            HeaderAction::Disconnect => self.disconnect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specification_cancel_cannot_cancel_a_manager_patrol() {
        let mut app = crate::app::root::board_tests::fixture();
        if let Screen::Connected(project) = &mut app.screen {
            project.activity.manager = Some(crate::app::manager::Manager::completed_for_test(Err(
                "test result is intentionally not polled".into(),
            )));
            project.activity.manager_progress.activity = Some("Manager is checking".into());
        } else {
            unreachable!();
        }
        let intent = Intent {
            cancel: true,
            ..Default::default()
        };
        app.apply_user_intent(intent);
        let Screen::Connected(project) = &app.screen else {
            unreachable!();
        };
        assert!(project.activity.manager.is_some());
        assert_eq!(
            project.activity.manager_progress.activity.as_deref(),
            Some("Manager is checking")
        );
        assert!(!app.specification_chat_busy());
        assert!(app.specification_chat_progress().is_none());
        assert!(app.manager_running());
        assert!(app.manager_progress().is_some());
    }
}
