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
mod task_cards;
mod task_details;
mod task_properties;
mod workspace_repositories;
mod workspace_tabs;

pub enum HeaderAction {
    Refresh,
    Import,
    Stakeholders,
    McpServers,
    CopySpec,
    OpenWorkspace,
    OpenRegisteredRepository { id: String },
    Disconnect,
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
pub(crate) use board_state::{planning_column, planning_parent_label, task_board_column};
pub(crate) use task_cards::task_key;
pub(crate) use task_properties::paint_task_properties;

#[cfg(test)]
mod tests;
