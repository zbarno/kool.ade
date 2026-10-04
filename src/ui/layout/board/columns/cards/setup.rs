use super::super::super::super::task_cards;
use super::super::super::super::*;

pub(in crate::ui::layout::board::columns) fn setup(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    issue: &crate::app::setup_attention::SetupIssue,
) {
    task_cards::board_card(ui, issue.id, None, false, true, false, |ui| {
        super::super::attention::badge(ui, super::super::attention::Kind::WaitingOnUser);
        ui.label(
            RichText::new("SETUP · NEEDS ATTENTION")
                .size(11.5)
                .color(theme::WARNING),
        );
        ui.label(RichText::new(issue.title).strong());
        ui.add(egui::Label::new(&issue.issue).wrap());
        ui.label(RichText::new("Why this matters").strong());
        ui.add(egui::Label::new(issue.why).wrap());
        ui.label(RichText::new("Kool.ad/e recommends").strong());
        ui.add(egui::Label::new(issue.recommendation).wrap());
        ui.add(egui::Label::new(issue.impact).wrap());
        ui.add(egui::Label::new(issue.next_action).wrap());
        if ui.button("Open settings").clicked() {
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("koolade_workspace_settings_open"), true)
            });
        }
        let label = if board.setup_checking {
            "Checking setup…"
        } else {
            "Retry setup check"
        };
        if ui
            .add_enabled(!board.setup_checking, egui::Button::new(label))
            .clicked()
        {
            s.dispatch(ApplicationCommand::RetrySetupCheck);
        }
        ui.label(
            RichText::new(issue.id)
                .size(11.0)
                .color(theme::TEXT_MUTED)
                .monospace(),
        );
    });
}
