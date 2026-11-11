//! The parent "Prepare this computer" card for the attention lane.
//!
//! Replaces the former stack of flat machine-setup cards with one grouped
//! checklist: overall progress, a phase sentence, flow controls, and a
//! row per step with its own state marker. Rows open the familiar per-step
//! detail view, where a consent offer is explained and can be accepted or
//! declined.

use super::super::super::*;
use crate::app::setup_workflow::can_request_fix;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::layout::board::columns) enum Marker {
    Complete,
    Working,
    ConsentNeeded,
    Barrier,
    Incomplete,
    Problem,
}

/// Pure row description so the presentation logic is testable without an
/// egui context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui::layout::board::columns) struct GroupRow {
    pub task_id: String,
    pub marker: Marker,
    pub title: String,
    pub subtitle: String,
    pub offers_fix: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::layout::board::columns) struct GroupSummary {
    /// Checks that currently verify clean.
    pub verified: u32,
    /// Total checks in the picture (verified + active).
    pub total: u32,
}

pub(in crate::ui::layout::board::columns) fn summarize(
    tasks: &[&crate::persistence::setup_tasks::LocalTask],
) -> GroupSummary {
    let verified = tasks
        .iter()
        .filter(|task| task.state == crate::persistence::setup_tasks::State::Verified)
        .count() as u32;
    let total = tasks
        .iter()
        .filter(|task| {
            task.is_active() || task.state == crate::persistence::setup_tasks::State::Verified
        })
        .count() as u32;
    GroupSummary { verified, total }
}

pub(in crate::ui::layout::board::columns) fn describe_row(
    task: &crate::persistence::setup_tasks::LocalTask,
) -> GroupRow {
    use crate::persistence::setup_tasks::State;
    let marker = match task.state {
        State::Verified | State::Superseded => Marker::Complete,
        State::InProgress | State::AwaitingVerification => Marker::Working,
        State::NeedsApproval => Marker::ConsentNeeded,
        State::BlockedByPrerequisite => Marker::Barrier,
        State::Failed | State::Interrupted | State::Cancelled | State::Unsupported => {
            Marker::Problem
        }
        _ => Marker::Incomplete,
    };
    GroupRow {
        task_id: task.id.clone(),
        marker,
        title: crate::ui::layout::setup_task_title(task),
        subtitle: task.state_label().to_owned(),
        offers_fix: can_request_fix(task),
    }
}

const MEMBER_ROW_CAP: usize = 12;

pub(in crate::ui::layout::board::columns) fn machine_group(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    tasks: &[&crate::persistence::setup_tasks::LocalTask],
    selected: &mut Option<String>,
) {
    let visible: Vec<GroupRow> = tasks.iter().map(|t| describe_row(t)).collect();
    if visible.is_empty() {
        return;
    }
    let flow = surface.setup_flow_position();
    let summary = summarize(tasks);
    let controls = flow_controls(flow.as_ref());
    let title_of = |id: &str| {
        visible
            .iter()
            .find(|row| row.task_id == id)
            .map(|row| row.title.clone())
            .unwrap_or_else(|| id.to_owned())
    };
    let open_details =
        task_cards::board_card(ui, "setup-machine-flow", None, false, true, false, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Prepare this computer").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(crate::ui::theme::metadata_text(format!(
                        "{} of {} checks ready",
                        summary.verified, summary.total
                    )))
                });
            });
            ui.label(crate::ui::theme::helper_text(phase_blurb(
                flow.as_ref(),
                title_of,
            )));
            ui.add_space(6.0);
            for row in visible.iter().take(MEMBER_ROW_CAP) {
                paint_member_row(ui, surface, row, selected);
            }
            if visible.len() > MEMBER_ROW_CAP {
                ui.label(crate::ui::theme::helper_text(format!(
                    "+{} more steps — open any row for details",
                    visible.len() - MEMBER_ROW_CAP
                )));
            }
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if controls.start
                    && ui
                        .add_enabled(
                            !board.setup_checking,
                            egui::Button::new("Fix it for me").fill(crate::ui::theme::ACTION),
                        )
                        .clicked()
                {
                    surface.dispatch(crate::ui::ApplicationCommand::StartSetupFlow);
                }
                if controls.pause_and_stop {
                    if ui.button("Pause").clicked() {
                        surface.dispatch(crate::ui::ApplicationCommand::PauseSetupFlow);
                    }
                    if ui.button("Stop").clicked() {
                        surface.dispatch(crate::ui::ApplicationCommand::StopSetupFlow);
                    }
                }
                let label = if board.setup_checking {
                    "Checking…"
                } else {
                    "Check again"
                };
                if ui
                    .add_enabled(!board.setup_checking, egui::Button::new(label))
                    .clicked()
                {
                    surface.dispatch(crate::ui::ApplicationCommand::RetrySetupCheck);
                }
            });
            ui.label(crate::ui::theme::helper_text(
                "Every step can be explained, approved, or declined on its own terms.",
            ));
        });
    if open_details {
        *selected = Some(visible[0].task_id.clone());
    }
}

fn paint_member_row(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    row: &GroupRow,
    selected: &mut Option<String>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(marker_color(row.marker), marker_glyph(row.marker));
        let emph = matches!(
            row.marker,
            Marker::ConsentNeeded | Marker::Barrier | Marker::Problem
        );
        let rt = egui::RichText::new(row.title.clone());
        let rt = if emph { rt.strong() } else { rt.weak() };
        let rt = if emph { rt.strong() } else { rt.weak() };
        let width = (ui.available_width() * 0.55).max(48.0);
        let control_height = ui.spacing().interact_size.y;
        let clicked = ui
            .add_sized([width, control_height], egui::Button::new(rt).frame(false))
            .clicked();
        if clicked {
            *selected = Some(row.task_id.clone());
        }
        ui.label(crate::ui::theme::metadata_text(&row.subtitle));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if row.offers_fix
                && !matches!(row.marker, Marker::Complete | Marker::Working)
                && ui.small_button("Fix").clicked()
            {
                surface.dispatch(crate::ui::ApplicationCommand::RequestSetupTaskFix {
                    task_id: row.task_id.clone(),
                });
            }
        });
    });
}

fn marker_glyph(marker: Marker) -> &'static str {
    match marker {
        Marker::Complete => "●",
        Marker::Working => "◌",
        Marker::ConsentNeeded => "◆",
        Marker::Barrier => "◐",
        Marker::Incomplete => "○",
        Marker::Problem => "▲",
    }
}

fn marker_color(marker: Marker) -> egui::Color32 {
    match marker {
        Marker::Complete => crate::ui::theme::SUCCESS,
        Marker::Working => crate::ui::theme::ACTION,
        Marker::ConsentNeeded | Marker::Problem => crate::ui::theme::WARNING,
        Marker::Barrier | Marker::Incomplete => crate::ui::theme::TEXT_DIM,
    }
}

#[cfg(test)]
#[path = "machine_group_tests.rs"]
mod tests;
