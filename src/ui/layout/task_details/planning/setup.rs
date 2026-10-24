use super::*;

pub(super) fn paint_machine_setup(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    task: &crate::persistence::setup_tasks::LocalTask,
) {
    ui.heading(&task.title);
    ui.label(crate::ui::theme::metadata_text(format!(
        "Machine setup · {:?}",
        task.state
    )));
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading(
        "Why this needs attention",
    ));
    ui.label(&task.explanation);
    if !task.prerequisites.is_empty() {
        ui.label(crate::ui::theme::section_heading("Waiting for"));
        for item in &task.prerequisites {
            ui.label(item);
        }
    }
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("What to do next"));
    ui.label(&task.next_step);
    ui.label(crate::ui::theme::helper_text(
        "Setup state stays on this machine, outside the project Git repository. Only verified Linux rootless image preparation is enabled here.",
    ));
    if task.id.starts_with("setup:images:general:") {
        surface.setup_image_preparation(ui, &task.id);
    }
    if ui.button("Open coding tools").clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_open_coding_settings"), true);
        });
    }
    let label = if board.setup_checking {
        "Checking setup…"
    } else {
        "Check machine again"
    };
    if ui
        .add_enabled(!board.setup_checking, egui::Button::new(label))
        .clicked()
    {
        surface.dispatch(crate::ui::ApplicationCommand::RetrySetupCheck);
    }
}

pub(super) fn paint_setup_issue(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    issue: &crate::app::setup_attention::SetupIssue,
) {
    ui.heading(issue.title);
    ui.label(crate::ui::theme::metadata_text("Setup · Needs your input"));
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("What needs attention"));
    ui.label(&issue.issue);
    ui.collapsing("Why this matters", |ui| ui.label(issue.why));
    ui.label(crate::ui::theme::section_heading("Recommended next step"));
    ui.label(issue.recommendation);
    ui.label(crate::ui::theme::helper_text(issue.impact));
    ui.label(issue.next_action);
    if ui.button("Open workspace settings").clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_workspace_settings_open"), true);
        });
    }
    let label = if board.setup_checking {
        "Checking setup…"
    } else {
        "Retry setup check"
    };
    if ui
        .add_enabled(
            !board.setup_checking,
            egui::Button::new(label).fill(crate::ui::theme::ACTION),
        )
        .clicked()
    {
        surface.dispatch(crate::ui::ApplicationCommand::RetrySetupCheck);
    }
}
