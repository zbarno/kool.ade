use super::super::{Dialog, KooladeApp, Screen};
use crate::ui::{HeaderAction, Intent, Surface};

impl KooladeApp {
    pub(super) fn apply_user_intent(&mut self, intent: Intent) {
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
            if let Screen::Connected(project) = &mut self.screen {
                project.activity.manager = None;
                if let Some(controller) = &project.active_turn {
                    controller.request_cancel();
                    self.toasts.warning("Cancellation requested…");
                }
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
                        &project.state.repo_root,
                        &project.state.workflow,
                    );
                    self.toasts.info("Git state refreshed");
                }
            }
            HeaderAction::Import => {
                self.dialog = Some(Dialog::Import(super::super::DlgImport::new()))
            }
            HeaderAction::Stakeholders => {
                if let Screen::Connected(project) = &self.screen {
                    self.dialog = Some(Dialog::Settings(super::super::DlgSettings::from_project(
                        project,
                    )));
                }
            }
            HeaderAction::McpServers => {
                if let Screen::Connected(project) = &self.screen {
                    self.dialog = Some(Dialog::Mcp(super::super::DlgMcp::from_project(project)));
                }
            }
            HeaderAction::CopySpec => self.copy_spec_to_clipboard(),
            HeaderAction::OpenWorkspace => self.open_workspace(),
            HeaderAction::OpenRegisteredRepository { id } => self.open_registered_repository(&id),
            HeaderAction::Disconnect => self.disconnect(),
        }
    }
}
