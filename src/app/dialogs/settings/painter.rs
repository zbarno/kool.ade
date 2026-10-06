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
    ui.horizontal(|ui| {
        ui.label(RichText::new("Teams").size(12.0).weak());
        ui.add_sized(
            egui::vec2(ui.available_width(), 26.0),
            TextEdit::singleline(&mut dlg.user_groups)
                .hint_text("Platform, QA  (comma separated)")
                .font(egui::FontId::proportional(12.5))
                .desired_width(320.0),
        );
    });
    ui.add_space(10.0);
    ui.label(
        RichText::new("Categories & owners — drives question routing")
            .size(13.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new("Choose existing people or teams, or enter new owners separated by commas.")
            .size(12.0)
            .weak(),
    );
    let owners = owner_choices(dlg);
    let mut removed: Vec<usize> = Vec::new();
    for (i, row) in dlg.rows.iter_mut().enumerate() {
        ui.push_id(("ownership_row", i), |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Category");
                    ui.add_sized(
                        [(ui.available_width() - 40.0).max(60.0), 28.0],
                        TextEdit::singleline(&mut row.category).id_salt("category"),
                    );
                    if crate::ui::overlays::close_button(ui)
                        .on_hover_text("Remove category")
                        .clicked()
                    {
                        removed.push(i);
                    }
                });
                ui.label(RichText::new("Owners").size(12.0).weak());
                ui.add(
                    TextEdit::singleline(&mut row.members)
                        .id_salt("owners")
                        .desired_width(f32::INFINITY)
                        .hint_text("Names or teams, separated by commas"),
                );
                egui::ComboBox::from_id_salt("existing_owners")
                    .selected_text("Select existing owners…")
                    .width(240.0_f32.min(ui.available_width()))
                    .show_ui(ui, |ui| {
                        if owners.is_empty() {
                            ui.label("Enter a name or team to make it available here.");
                        }
                        for owner in &owners {
                            let mut selected = csv_parts(&row.members)
                                .iter()
                                .any(|value| value.eq_ignore_ascii_case(owner));
                            if ui.checkbox(&mut selected, owner).changed() {
                                set_owner_selected(&mut row.members, owner, selected);
                            }
                        }
                    });
            });
            ui.add_space(6.0);
        });
    }
    for idx in removed.iter().rev() {
        if *idx < dlg.rows.len() {
            dlg.rows.remove(*idx);
        }
    }
    if ui.button("+ add category").clicked() {
        dlg.rows.push(Row {
            category: String::new(),
            members: String::new(),
        });
    }
    ui.add_space(8.0);
    super::repository_names::paint(ui, dlg);
    ui.add_space(12.0);
    ui.separator();
    if ui.button("Configure coding tools…").clicked() {
        dlg.open_harness_setup = true;
    }
    ui.collapsing("AI harness setup", |ui| paint_harness_guide(ui, dlg));
    ui.add_space(6.0);
    footers(ui, &dlg.feedback)
}
