use super::super::super::super::task_cards;
use super::super::super::super::*;

pub(in crate::ui::layout::board::columns) fn setup(
    ui: &mut egui::Ui,
    _s: &mut dyn Surface,
    _board: &crate::ui::planning_board::ViewModel,
    issue: &crate::app::setup_attention::SetupIssue,
    selected: &mut Option<String>,
) {
    let open_details = task_cards::board_card(ui, issue.id, None, false, true, false, |ui| {
        super::super::attention::badge(ui, super::super::attention::Kind::WaitingOnUser);
        if ui
            .add(
                egui::Button::new(RichText::new(issue.title).strong())
                    .frame(false)
                    .wrap(),
            )
            .clicked()
        {
            *selected = Some(issue.id.to_owned());
        }
        ui.add(egui::Label::new(&issue.issue).truncate());
        ui.label(theme::helper_text("Setup needs your attention"));
        if ui.button("Open details").clicked() {
            *selected = Some(issue.id.to_owned());
        }
    });
    if open_details {
        *selected = Some(issue.id.to_owned());
    }
}
