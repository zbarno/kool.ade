use super::super::super::super::*;

pub(in crate::ui::layout::board::columns) fn approval(
    ui: &mut egui::Ui,
    action: &crate::ui::feature_approval::Action,
    selected: &mut Option<String>,
) {
    let title = action
        .specification
        .lines()
        .find(|line| line.starts_with('#'))
        .map(|line| line.trim_start_matches('#').trim())
        .unwrap_or(&action.id);
    let open = crate::ui::layout::task_cards::board_card(
        ui,
        &format!("feature-approval:{}", action.id),
        None,
        false,
        true,
        false,
        |ui| {
            ui.horizontal(|ui| {
                theme::badge(ui, "Specification", theme::PANEL, theme::TEXT_DIM);
                super::super::attention::badge(ui, super::super::attention::Kind::WaitingOnUser);
            });
            ui.label(RichText::new(title).strong().color(theme::TEXT));
            if ui.button("Review specification").clicked() {
                *selected = Some(format!("feature-approval:{}", action.id));
            }
        },
    );
    if open {
        *selected = Some(format!("feature-approval:{}", action.id));
    }
}
