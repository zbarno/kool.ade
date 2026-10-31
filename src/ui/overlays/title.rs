//! Modal heading placement with a natural text layout and a drag target.
use super::*;

pub(super) fn paint(ui: &mut egui::Ui, title: &str, width: f32) -> egui::Response {
    if title == "Task details" {
        ui.allocate_ui_with_layout(
            egui::vec2((width - 38.0).max(20.0), 36.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2((width - 38.0).max(20.0), 36.0));
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .font(theme::task_heading_font(ui.ctx(), 24.0))
                            .color(theme::TEXT),
                    )
                    .sense(egui::Sense::drag())
                    .truncate(),
                )
            },
        )
        .inner
        .on_hover_text(title)
    } else {
        ui.add_sized(
            [(width - 38.0).max(20.0), 28.0],
            egui::Label::new(RichText::new(title).strong().size(17.0))
                .halign(egui::Align::Min)
                .sense(egui::Sense::drag())
                .truncate(),
        )
        .on_hover_text(title)
    }
}

#[cfg(test)]
mod tests;
