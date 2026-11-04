use super::*;

impl KooladeApp {
    pub(super) fn paint_project_entry(&mut self, ui: &mut egui::Ui, next: &mut Option<Step>) {
        presentation::problem(ui, self.conn_error.as_deref());
        if self.onboarding.step == Step::Projects {
            ui.heading("Open a code repository");
            ui.label("Clone a code repository from GitHub or open a local checkout.");
            if ui
                .add(presentation::primary_button(
                    "Clone a code repository from GitHub…",
                ))
                .clicked()
            {
                self.wizard_open(crate::app::root::WelcomeEntry::CloneRepository);
                *next = Some(Step::Advanced);
            }
            if ui.button("Open a local code repository…").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                self.finish_connect(welcome::attempt_connect(&path.to_string_lossy()));
            }
            if ui.button("Review computer setup").clicked() {
                *next = Some(Step::Readiness);
            }
        }
    }
}
