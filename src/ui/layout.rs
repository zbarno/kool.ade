//! Workspace screen coordinator.
use crate::ui::{ApplicationCommand, Surface, theme};
use egui::{CentralPanel, Frame, Layout, Panel, RichText};

mod activity;
mod board;
mod board_state;
pub(crate) mod brand;
mod chat_panel;
mod header;
mod new_task;
mod settings;
mod setup_welcome;
mod task_cards;
mod task_details;
mod task_properties;
mod workspace_repositories;
mod workspace_tabs;

pub(crate) use setup_welcome::{
    WelcomeSetupAction, paint_board as paint_welcome_setup_board,
    paint_details as paint_welcome_setup_details,
};

pub(crate) fn setup_task_summary(
    task: &crate::persistence::setup_tasks::LocalTask,
) -> &'static str {
    task_details::machine_setup_summary(task)
}

pub(crate) fn setup_task_title(task: &crate::persistence::setup_tasks::LocalTask) -> String {
    task_details::machine_setup_title(task)
}

pub enum HeaderAction {
    Refresh,
    Import,
    Stakeholders,
    McpServers,
    CopySpec,
    OpenWorkspace,
    OpenRegisteredRepository { id: String },
    AddCodeRepository,
    MigrateEmbeddedPlanning,
    RollbackPlanningMigration,
    Disconnect,
}

/// Opaque one-shot proof emitted only by the explicit custom-target approval
/// button. Its constructor stays private to this UI module and descendants.
#[derive(Clone)]
pub struct RuntimeTargetApprovalProof {
    nonce: uuid::Uuid,
}

impl RuntimeTargetApprovalProof {
    fn from_user_approval_click() -> Self {
        Self {
            nonce: uuid::Uuid::new_v4(),
        }
    }

    pub(crate) fn receipt_uid(&self) -> String {
        self.nonce.hyphenated().to_string()
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::from_user_approval_click()
    }
}

pub fn paint(ui: &mut egui::Ui, s: &mut dyn Surface) {
    s.dispatch(ApplicationCommand::DrainTaskChatSaves);
    let board = s.planning_board();
    let compact = ui.ctx().content_rect().width() < 960.0;
    let settings_id = egui::Id::new("koolade_workspace_settings_open");
    let mut settings_open = ui
        .ctx()
        .data_mut(|d| d.get_temp::<bool>(settings_id).unwrap_or(false));
    let settings_was_open = settings_open;
    header::paint(ui, s, compact, &mut settings_open);
    chat_panel::paint(ui, s, &board, compact);
    workspace_tabs::paint(ui, s, &board, compact);
    settings_open = settings::paint(ui, s, &board, settings_open, settings_was_open);
    ui.ctx()
        .data_mut(|d| d.insert_temp(settings_id, settings_open));
}

#[cfg(test)]
pub(crate) use activity::task_card_activity_band;
pub(crate) use board_state::{planning_column, planning_parent_label, task_document_board_column};
pub(crate) use task_cards::task_key;
pub(crate) use task_properties::paint_task_properties;

#[cfg(test)]
pub(crate) fn paint_custom_runtime_target_action(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
) {
    let height = ui.available_height();
    task_details::paint(ui, surface, doc, height, &mut None);
}

#[cfg(test)]
pub(crate) fn seed_custom_runtime_target_repository_root_draft(
    context: &egui::Context,
    ticket: &str,
    app_surface: crate::project_runtime::AppSurface,
) {
    task_details::seed_custom_target_repository_root_draft(context, ticket, app_surface);
}

#[cfg(test)]
mod tests;
