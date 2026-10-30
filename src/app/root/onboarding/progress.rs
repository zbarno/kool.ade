use super::*;

impl KooladeApp {
    pub(super) fn paint_onboarding_progress(&self, ui: &mut egui::Ui) {
        if self.onboarding.step == Step::Start {
            return;
        }
        let stage = match self.onboarding.step {
            Step::Manager => 0,
            Step::Readiness => 1,
            Step::ReviewProject => 3,
            _ => 2,
        };
        let labels = [
            "Meet your Man.ager",
            "Prepare this computer",
            "Choose a project",
            "Review and open",
        ];
        if ui.available_width() >= 1100.0 {
            egui::Panel::left("onboarding_progress")
                .exact_size(200.0)
                .resizable(false)
                .show(ui, |ui| {
                    ui.add_space(30.0);
                    ui.heading("Getting started");
                    for (index, label) in labels.iter().enumerate() {
                        ui.add_space(16.0);
                        let label = format!("{} · {label}", index + 1);
                        if index == stage {
                            ui.strong(label);
                        } else {
                            ui.label(label);
                        }
                    }
                });
        } else {
            egui::Panel::top("onboarding_progress_compact").show(ui, |ui| {
                ui.label(format!("Step {} of 4 · {}", stage + 1, labels[stage]));
            });
        }
    }
}
