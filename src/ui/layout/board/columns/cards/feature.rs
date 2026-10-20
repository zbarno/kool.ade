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
    let key = action.board_key();
    let open =
        crate::ui::layout::task_cards::board_card(ui, &key, None, false, true, false, |ui| {
            ui.horizontal(|ui| {
                theme::badge(ui, "Plan confirmation", theme::PANEL, theme::TEXT_DIM);
                super::super::attention::badge(ui, super::super::attention::Kind::WaitingOnUser);
            });
            ui.label(RichText::new(title).strong().color(theme::TEXT));
            if ui.button("Review specification").clicked() {
                *selected = Some(key.clone());
            }
        });
    if open {
        *selected = Some(key);
    }
}
