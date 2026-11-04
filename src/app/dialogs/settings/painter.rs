use super::*;

/// Paint card body; returns (save_pressed, close_pressed).
pub fn paint_import_card(ui: &mut egui::Ui, dlg: &mut DlgImport) -> (bool, bool) {
    ui.label(RichText::new("Add reference material").size(12.5).weak());
    ui.add_space(3.0);
    ui.label(
        RichText::new("Paste file or folder paths (one per line). Existing content under planning/ is ignored.")
            .weak()
            .size(11.0),
    );
    ui.add_space(4.0);
    ui.add_sized(
        egui::vec2(ui.available_width(), 116.0),
        TextEdit::multiline(&mut dlg.paths)
            .hint_text("/path/to/prd.pdf\n/path/to/architecture-notes/")
            .font(egui::FontId::monospace(12.0))
            .desired_width(f32::INFINITY)
            .desired_rows(5),
    );
    footers(ui, &dlg.feedback)
}

/// Paint card body; returns (save_pressed, close_pressed).
pub fn paint_settings_card(ui: &mut egui::Ui, dlg: &mut DlgSettings) -> (bool, bool) {
    ui.label(
        RichText::new("Who am I?")
            .size(13.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.add_space(3.0);
    ui.label(RichText::new(&dlg.identity_note).size(11.0).weak());
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Name").size(12.0).weak());
        ui.add_sized(
            egui::vec2(ui.available_width(), 26.0),
            TextEdit::singleline(&mut dlg.user_name)
                .font(egui::FontId::proportional(12.5))
                .desired_width(240.0),
        );
    });
    ui.add_space(6.0);
    footers(ui, &dlg.feedback)
}
