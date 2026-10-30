use super::*;
use crate::app::root::bootstrap_conversation::{Context, ONBOARDING_CHAT};

impl KooladeApp {
    /// Narrow layouts use one full-width help sheet, preserving the wizard
    /// beneath it without obscured controls or a second keyboard focus scope.
    pub(super) fn paint_onboarding_help(&mut self, ui: &mut egui::Ui) -> bool {
        if !self.onboarding.help_open {
            return false;
        }
        if ui.available_width() >= 850.0 {
            egui::Panel::right("onboarding_help")
                .default_size(300.0)
                .resizable(true)
                .show(ui, |ui| {
                    presentation::configure(ui);
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| self.paint_onboarding_help_content(ui));
                });
            false
        } else {
            let mut close = ui.input(|input| input.key_pressed(egui::Key::Escape));
            egui::CentralPanel::default().show(ui, |ui| {
                presentation::configure(ui);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    close |= ui.button("Return to this step").clicked();
                    self.paint_onboarding_help_content(ui);
                });
            });
            if close {
                self.onboarding.help_open = false;
                self.onboarding.save();
                ui.ctx().request_repaint();
            }
            true
        }
    }

    fn paint_onboarding_help_content(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ask Man.ager");
        ui.label("Questions about this step? Start with the guide or ask your selected helper.");
        ui.collapsing("Guide to this step · available offline", |ui| {
            ui.label(Context::onboarding(&self.onboarding, &self.machine_setup_tasks).guidance());
        });
        ui.label("Chat stays on this computer. Sending uses your chosen provider; leave out passwords and keys.");
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
        } else {
            let ready = readiness::helper_ready(&crate::persistence::harness_settings::load().0);
            if ui
                .add_enabled(
                    ready,
                    presentation::primary_button("Send to my selected helper"),
                )
                .on_disabled_hover_text(
                    "Choose a setup helper and verify its sign-in and model first.",
                )
                .clicked()
            {
                self.send_onboarding_message();
            }
            if !ready {
                ui.label("AI help is not verified yet. The offline guide above remains available.");
            }
        }
        if ui.button("Choose or check my AI tool").clicked() {
            self.dialog = Some(Dialog::HarnessSetup(app_dialogs::DlgHarnessSetup::new()));
        }
    }
}
