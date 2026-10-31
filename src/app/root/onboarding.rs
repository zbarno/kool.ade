use super::*;
mod launcher;
mod opening;
pub(super) use launcher::Launcher;
pub(super) use opening::OpenJob;
mod help;
mod presentation;
mod progress;
mod project;
mod readiness;
mod recovery;
use crate::app::welcome::onboarding::Step;
pub(super) use recovery::Recovery;
impl KooladeApp {
    /// Returns false for the existing workspace/advanced surface.
    pub(super) fn paint_onboarding(&mut self, ui: &mut egui::Ui) -> bool {
        self.poll_onboarding_creation();
        self.poll_onboarding_open();
        if self.onboarding_open.is_some() {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.spinner(); ui.heading("Opening your workspace…");
                ui.label("Loading and checking its existing history. Closing the app may interrupt loading; reopen the same workspace to resume safely.");
            });
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            return true;
        }
        if self.onboarding.step == Step::Projects {
            self.onboarding_launcher.poll(ui);
        }
        if !matches!(self.screen, Screen::Welcome) {
            return false;
        }
        if self.onboarding.step != Step::Advanced {
            self.paint_onboarding_progress(ui);
        } else {
            egui::Panel::top("advanced_onboarding_help").show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Need help? Ask Man.ager").clicked() {
                        self.onboarding.help_open = !self.onboarding.help_open;
                        self.onboarding.save();
                    }
                    if ui.button("Guided welcome").clicked() {
                        self.onboarding.go(Step::Projects);
                    }
                });
            });
        }
        if self.paint_onboarding_help(ui) {
            self.render_queued_settings_dialog(ui);
            self.toasts.show(ui.ctx());
            return true;
        }
        if self.onboarding.step == Step::Advanced {
            return false;
        }
        let mut next = None;
        let mut configure = false;
        let selection_id = egui::Id::new("koolade_welcome_setup_selection");
        let mut selection = ui
            .ctx()
            .data_mut(|data| data.get_temp::<String>(selection_id))
            .or_else(|| self.onboarding.active_setup_task_id.clone());
        let mut action = crate::ui::layout::WelcomeSetupAction::default();
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                presentation::configure(ui);
                if ui.button("Need help? Ask Man.ager").clicked() {
                    self.onboarding.help_open = !self.onboarding.help_open;
                    self.onboarding.save();
                }
                ui.add_space(16.0);
                presentation::card(ui, |ui| {
                ui.label(egui::RichText::new("Kool.ad/e").strong().color(crate::ui::theme::TEXT_DIM));
                ui.add_space(12.0);
                if let Some(error) = &self.onboarding.error { ui.label(error); }
                match self.onboarding.step {
                    Step::Start => {
                        ui.label(crate::ui::theme::page_title("Tell Kool.ad/e what you want to make."));
                        ui.label("Man.ager will help get your computer ready and guide you into your project. You decide what changes.");
                        if ui.add(presentation::primary_button("Get started")).clicked() { next = Some(Step::Manager); }
                    }
                    Step::Manager => {
                        ui.heading("Choose your AI assistant");
                        ui.label("Man.ager helps plan your project, answer questions and coordinate work. Choose which AI tool powers it. You can change this later.");
                        self.paint_manager_choice_status(ui);
                        if ui.add(presentation::primary_button("Choose and verify my AI tool")).clicked() { configure = true; }
                        if ui.button("Continue setup").clicked() { next = Some(Step::Readiness); }
                        if ui.button("Back").clicked() { next = Some(Step::Start); }
                    }
                    Step::Readiness => {
                        ui.heading("Let's get ready");
                        ui.label("Planning and running code have different requirements. Each repair needs your permission; choosing to continue does not enable coding execution.");
                        self.paint_readiness_summary(ui, &mut selection);
                        ui.collapsing("Review checks and available repairs", |ui| {
                            action = crate::ui::layout::paint_welcome_setup_board(ui,
                                &self.machine_setup_tasks, &self.setup_attention,
                                self.setup_probe.is_some(), &mut selection);
                        });
                        if ui.add(presentation::primary_button("Continue to my projects")).clicked() { next = Some(Step::Projects); }
                        if ui.button("Back").clicked() { next = Some(Step::Manager); }
                    }
                    Step::Projects | Step::NewProject | Step::ReviewProject => {
                        self.paint_project_entry(ui, &mut next);
                    }
                    _ => {}
                }
                ui.add_space(24.0);
                if self.onboarding_creation.is_none() && ui.button("Advanced options").clicked() { next = Some(Step::Advanced); }
                });
            });
        });
        if action.retry {
            self.dispatch_ui_command(crate::ui::ApplicationCommand::RetrySetupCheck);
        }
        if let Some(task_id) = action.fix_task {
            self.onboarding.pending_operation_task_id = Some(task_id.clone());
            self.onboarding.save();
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
        if self.onboarding.active_setup_task_id != selection {
            self.onboarding.active_setup_task_id = selection.clone();
            self.onboarding.save();
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
