use super::super::*;

pub(super) fn paint(ui: &mut egui::Ui, column: usize, label: &str, count: usize) {
    ui.horizontal(|ui| {
        let lane_color = theme::TEXT_DIM;
        let (dot, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(dot.center(), 6.0, lane_color.gamma_multiply(0.2));
        ui.painter().circle_filled(dot.center(), 3.5, lane_color);
        ui.label(RichText::new(label).size(15.0).strong().color(theme::TEXT));
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            theme::badge(
                ui,
                &format!("{count}"),
                lane_color.gamma_multiply(0.12),
                lane_color,
            );
        });
    });
    let subtitle = ui.label(
        RichText::new(match column {
            0 => "Ready for the next step",
            1 => "Ideas becoming reality",
            2 => "A fresh pair of eyes",
            3 => "Your input moves things forward",
            _ => "Look what you made happen",
        })
        .size(11.0)
        .color(theme::TEXT_MUTED),
    );
    if column == 3 {
        subtitle.on_hover_text(
            "Most recent task or conversation activity appears first. Cards without a recorded time keep a stable order.",
        );
    }
    let (line, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 2.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(line, 1, theme::BORDER.gamma_multiply(0.65));
}
