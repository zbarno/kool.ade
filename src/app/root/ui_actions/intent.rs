use super::super::{Dialog, PacketApp, Screen};
use crate::ui::{HeaderAction, Intent, Surface};

impl PacketApp {
    pub(super) fn apply_user_intent(&mut self, intent: Intent) {
        if let Some(id) = &intent.approve_feature {
            self.approve_and_prepare_feature(id);
            return;
        }
        if intent.implement_tasks {
            if self.implementation_offer() {
                super::super::requested_action::start_next(self);
            }
            return;
        }
        if intent.generate_tasks {
            if self.task_offer().is_some() {
                if let Screen::Connected(project) = &self.screen
                    && let Some((id, _)) = &project.state.active_feature
                {
                    let id = id.clone();
                    self.approve_and_prepare_feature(&id);
                    return;
                }
                self.start_turn_with_purpose(
                    "Generate task stories for the current reviewed specification.",
                    crate::core::workflow::TurnPurpose::GenerateTasks,
                );
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
            HeaderAction::Disconnect => self.disconnect(),
        }
    }
}
