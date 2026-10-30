use crate::ui::theme;

pub(super) fn configure(ui: &mut egui::Ui) {
    ui.spacing_mut().interact_size.y = 44.0;
    ui.spacing_mut().button_padding = egui::vec2(14.0, 10.0);
    ui.spacing_mut().item_spacing.y = 10.0;
    ui.spacing_mut().text_edit_width = ui.available_width().min(560.0);
}

pub(super) fn primary_button(label: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(label.into())
            .strong()
            .color(theme::TEXT),
    )
    .fill(theme::ACTION)
    .min_size(egui::vec2(180.0, 44.0))
}

pub(super) fn card(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width().min(680.0);
    let inset: i8 = if width < 420.0 { 14 } else { 26 };
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        egui::Frame::new()
            .fill(theme::PANEL)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(12)
            .inner_margin(inset)
            .show(ui, |ui| {
                ui.set_width((width - f32::from(inset) * 2.0).max(180.0));
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), contents);
            });
    });
}

pub(super) fn problem(ui: &mut egui::Ui, detail: Option<&str>) {
    let Some(detail) = detail else {
        return;
    };
    ui.strong("This step needs attention");
    ui.label("Any completed work is kept. Nothing will be deleted automatically. Check the location and access, then review the same request to retry, or choose another folder.");
    ui.collapsing("Technical details", |ui| {
        ui.label(crate::error::redact_secrets(detail));
    });
}

pub(super) fn single_line(
    ui: &mut egui::Ui,
    value: &mut String,
    label: egui::Id,
) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(f32::INFINITY)
            .min_size(egui::vec2(0.0, 44.0))
            .margin(egui::vec2(8.0, 12.0)),
    )
    .labelled_by(label)
}
