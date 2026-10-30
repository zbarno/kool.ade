use super::*;
mod existing;
mod help;
mod progress;
mod project;
use crate::app::welcome::onboarding::Step;
pub(super) type InspectionResult = (
    String,
    std::path::PathBuf,
    Result<crate::core::managed_workspace::ExistingProjectKind, String>,
);

impl KooladeApp {
    /// Returns false for the existing workspace/advanced surface.
    pub(super) fn paint_onboarding(&mut self, ui: &mut egui::Ui) -> bool {
        self.poll_onboarding_creation();
        if !matches!(self.screen, Screen::Welcome) {
            return false;
        }
        if matches!(self.onboarding.step, Step::Advanced) {
            return false;
        }
        self.paint_onboarding_progress(ui);
        self.paint_onboarding_help(ui);
        let mut next = None;
        let mut configure = false;
        let selection_id = egui::Id::new("koolade_welcome_setup_selection");
        let mut selection = ui
            .ctx()
            .data_mut(|data| data.get_temp::<String>(selection_id));
        let mut action = crate::ui::layout::WelcomeSetupAction::default();
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if ui.button("Need help? Ask Man.ager").clicked() {
                    self.onboarding.help_open = !self.onboarding.help_open;
                    self.onboarding.go(self.onboarding.step);
                }
                ui.add_space(24.0);
                ui.heading("Kool.ad/e");
                ui.label("Your idea. A clear plan. A place to start.");
                ui.add_space(24.0);
                if let Some(error) = &self.onboarding.error { ui.label(error); }
                match self.onboarding.step {
                    Step::Start => {
                        ui.heading("Tell Kool.ad/e what you want to make.");
                        ui.label("Man.ager will help get your computer ready and guide you into your project. You decide what changes.");
                        if ui.button("Get started").clicked() { next = Some(Step::Manager); }
                    }
                    Step::Manager => {
                        ui.label("1 of 4 · Meet your Man.ager");
                        ui.heading("Choose your AI assistant");
                        ui.label("Man.ager helps plan your project, answer questions and coordinate work. Choose which AI tool powers it. You can change this later.");
                        if ui.button("Choose and verify my AI tool").clicked() { configure = true; }
                        if ui.link("Continue setup").clicked() { next = Some(Step::Readiness); }
                        if ui.link("Back").clicked() { next = Some(Step::Start); }
                    }
                    Step::Readiness => {
                        ui.label("2 of 4 · Prepare this computer");
                        ui.heading("Let's get ready");
                        ui.label("Planning and running code have different requirements. Each repair needs your permission; choosing to continue does not enable coding execution.");
                        if self.setup_probe.is_some() { ui.spinner(); ui.label("Checking this computer…"); }
                        ui.collapsing("Review checks and available repairs", |ui| {
                            action = crate::ui::layout::paint_welcome_setup_board(ui,
                                &self.machine_setup_tasks, &self.setup_attention,
                                self.setup_probe.is_some(), &mut selection);
                        });
                        if ui.button("Continue to my projects").clicked() { next = Some(Step::Projects); }
                        if ui.link("Back").clicked() { next = Some(Step::Manager); }
                    }
                    Step::ExistingProject => { self.paint_existing_project(ui, &mut next); }
                    Step::Projects | Step::NewProject | Step::ReviewProject => {
                        self.paint_project_entry(ui, &mut next);
                    }
                    _ => {}
                }
                ui.add_space(24.0);
                ui.collapsing("Need help?", |ui| {
                    ui.label("You can explore planning before setting up code execution. Use the setup details to review a problem and ask for guidance. No repair runs just because you move to the next step.");
                });
                if self.onboarding_creation.is_none() && ui.link("Advanced options").clicked() { next = Some(Step::Advanced); }
            });
        });
        if action.retry {
            self.dispatch_ui_command(crate::ui::ApplicationCommand::RetrySetupCheck);
        }
        if let Some(task_id) = action.fix_task {
            self.dispatch_ui_command(crate::ui::ApplicationCommand::RequestSetupTaskFix {
                task_id,
            });
        }
        if let Some(task) = selection.as_deref() {
            let tasks = self.machine_setup_tasks.clone();
            let notices = self.setup_attention.clone();
            let checking = self.setup_probe.is_some();
            if crate::ui::layout::paint_welcome_setup_details(
                ui, self, &tasks, &notices, checking, task,
            ) {
                selection = None;
            }
        }
        ui.ctx().data_mut(|data| {
            if let Some(selection) = selection {
                data.insert_temp(selection_id, selection);
            } else {
                data.remove::<String>(selection_id);
            }
        });
        if configure {
            self.dialog = Some(Dialog::HarnessSetup(app_dialogs::DlgHarnessSetup::new()));
        }
        if let Some(step) = next {
            self.onboarding.go(step);
        }
        self.render_queued_settings_dialog(ui);
        self.toasts.show(ui.ctx());
        true
    }
}
