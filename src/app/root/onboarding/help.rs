use super::*;
use crate::app::root::bootstrap_conversation::{Context, ONBOARDING_CHAT};

impl KooladeApp {
    pub(super) fn paint_onboarding_help(&mut self, ui: &mut egui::Ui) {
        if !self.onboarding.help_open {
            return;
        }
        if ui.available_width() >= 850.0 {
            egui::Panel::right("onboarding_help")
                .default_size(290.0)
                .resizable(true)
                .show(ui, |ui| self.paint_onboarding_help_content(ui));
        } else {
            let mut open = self.onboarding.help_open;
            egui::Window::new("Ask Man.ager")
                .id(egui::Id::new("onboarding_help_window"))
                .open(&mut open)
                .default_width((ui.available_width() - 40.0).max(220.0))
                .show(ui.ctx(), |ui| self.paint_onboarding_help_content(ui));
            if open != self.onboarding.help_open {
                self.onboarding.help_open = open;
                self.onboarding.go(self.onboarding.step);
            }
        }
    }

    fn paint_onboarding_help_content(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ask Man.ager");
        ui.label(Context::onboarding(&self.onboarding, &self.machine_setup_tasks).guidance());
        ui.separator();
        ui.label("Chat stays on this computer. Sending uses your selected AI provider. Do not include passwords, keys or private project content.");
        let view = self.bootstrap_conversation.view(ONBOARDING_CHAT);
        egui::ScrollArea::vertical()
            .id_salt("onboarding_messages")
            .max_height(260.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for message in view.messages {
                    ui.label(match message.role {
                        crate::domain::ChatRole::User => "You",
                        crate::domain::ChatRole::Agent => "Man.ager",
                        crate::domain::ChatRole::System => "Status",
                    });
                    ui.label(message.text);
                    ui.add_space(8.0);
                }
            });
        if let Some(error) = view.error {
            ui.label(error);
        }
        if let Some(draft) = self.bootstrap_conversation.draft_mut(ONBOARDING_CHAT) {
            ui.add(
                egui::TextEdit::multiline(draft)
                    .desired_rows(3)
                    .hint_text("What would you like explained?")
                    .desired_width(f32::INFINITY),
            );
        }
        if view.busy {
            ui.spinner();
            if ui.button("Cancel response").clicked() {
                self.cancel_bootstrap_conversation(ONBOARDING_CHAT);
            }
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        } else if ui.button("Send to my selected helper").clicked() {
            self.send_onboarding_message();
        }
        if ui.link("Choose or check my AI tool").clicked() {
            self.dialog = Some(Dialog::HarnessSetup(app_dialogs::DlgHarnessSetup::new()));
        }
    }
}
