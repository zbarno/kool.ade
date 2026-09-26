//! Shared rendering for issue-specific choices in planning and task blockers.
use crate::ui::theme;
use egui::RichText;

pub(crate) fn paint(
    ui: &mut egui::Ui,
    button_label: &str,
    meaning: &str,
    consequence: Option<&str>,
    enabled: bool,
) -> bool {
    egui::Frame::NONE
        .fill(theme::PANEL_ALT)
        .corner_radius(6)
        .inner_margin(8)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let clicked = ui
                .add_enabled(
                    enabled,
                    egui::Button::new(RichText::new(button_label).strong()).wrap(),
                )
                .clicked();
            ui.add(egui::Label::new(meaning).wrap());
            if let Some(consequence) = consequence {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("If chosen: {consequence}"))
                            .small()
                            .color(theme::TEXT_DIM),
                    )
                    .wrap(),
                );
            }
            clicked
        })
        .inner
}
