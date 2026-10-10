#[derive(Clone, Default)]
pub struct DlgCodeRepository {
    pub id: String,
    pub role: String,
    pub remote: String,
    pub checkout: String,
    pub feedback: Option<(bool, String)>,
}

impl DlgCodeRepository {
    pub fn paint(&mut self, ui: &mut egui::Ui) -> (bool, bool, bool) {
        ui.label(crate::ui::theme::section_heading("Add a code repository"));
        ui.label("The repository identity is shared in planning files; its checkout path stays on this computer.");
        ui.horizontal(|ui| {
            ui.label("Stable ID");
            ui.add_sized(
                [ui.available_width() - 74.0, 28.0],
                egui::TextEdit::singleline(&mut self.id)
                    .hint_text("lowercase-id")
                    .char_limit(40),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Role");
            ui.add_sized(
                [ui.available_width() - 74.0, 28.0],
                egui::TextEdit::singleline(&mut self.role)
                    .hint_text("What this repository contains")
                    .char_limit(120),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Remote (optional)");
            ui.add_sized(
                [ui.available_width() - 116.0, 28.0],
                egui::TextEdit::singleline(&mut self.remote)
                    .hint_text("https://github.com/owner/repository.git")
                    .char_limit(300),
            );
        });
        let mut browse = false;
        ui.horizontal(|ui| {
            ui.add_sized(
                [ui.available_width() - 122.0, 28.0],
                egui::TextEdit::singleline(&mut self.checkout)
                    .hint_text("Path to an existing Git checkout"),
            );
            if ui.button("Browse…").clicked() {
                browse = true;
            }
        });
        if let Some((ok, message)) = &self.feedback {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(message).color(if *ok {
                crate::ui::theme::SUCCESS
            } else {
                crate::ui::theme::DANGER
            }));
        }
        let mut register = false;
        let mut cancel = false;
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
            if ui
                .add(egui::Button::new("Register").fill(crate::ui::theme::ACCENT_SOFT))
                .clicked()
            {
                register = true;
            }
        });
        (register, cancel, browse)
    }
}
