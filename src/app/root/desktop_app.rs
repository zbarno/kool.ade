use super::*;
use eframe::{App, Frame};

mod welcome_options;
use welcome_options::{paint_more_connections, paint_welcome_primary};

impl App for KooladeApp {
    /// F7 accrual (AD-4): settle any still-open agent intervals as
    /// `InterruptedDiscard` before the ledger disappears with the process.
    fn on_exit(&mut self) {
        crate::core::time_accrual::app_close_flush();
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.5);
        self.tick(dt, ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        self.paint_screen(ui);
    }
}

impl KooladeApp {
    pub(super) fn paint_screen(&mut self, ui: &mut egui::Ui) {
        let mut pending = Vec::new();
        for job in self.launch_history_jobs.drain(..) {
            if job.is_finished() {
                if !matches!(job.join(), Ok(Ok(()))) {
                    self.toasts
                        .warning("Could not save recent workspace history.");
                }
                self.onboarding_launcher.invalidate();
            } else {
                pending.push(job);
            }
        }
        self.launch_history_jobs = pending;
        if !self.launch_history_jobs.is_empty() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        crate::ui::theme::apply(ui.ctx());
        let welcome_screen = matches!(&self.screen, Screen::Welcome);
        if welcome_screen && self.paint_onboarding(ui) {
            return;
        }
        if !welcome_screen {
            self.paint_onboarding_handoff(ui);
        }
        let selection_id = egui::Id::new("koolade_welcome_setup_selection");
        let mut welcome_selection = ui
            .ctx()
            .data_mut(|data| data.get_temp::<String>(selection_id));
        let mut welcome_setup_action = crate::ui::layout::WelcomeSetupAction::default();

        match &mut self.screen {
            Screen::Welcome => {
                let harness_setup_slot = std::cell::RefCell::new(false);
                let local_workspaces =
                    crate::core::managed_workspace::WorkspaceManager::from_state_root()
                        .list_launcher()
                        .map(|launcher| launcher.available);
                let mut local_workspaces = local_workspaces.unwrap_or_default();
                // A damaged personal history must not hide otherwise usable workspaces.
                let _ = crate::core::managed_workspace::WorkspaceManager::from_state_root()
                    .sort_by_recent_open(&mut local_workspaces);
                if self.conn_workspace_id.is_none() {
                    self.conn_workspace_id = local_workspaces.first().map(|workspace| workspace.id);
                }
                // In-flight badge for the card: ("github.com/{o}/{r}", repo).
                let cloning = self
                    .clone_job
                    .as_ref()
                    .map(|j| (j.url_display.clone(), j.repo.clone()));
                // Copy for the modal overlay (the tuple below is moved into
                // `paint_more_connections`).
                // Owned copy for the modal overlay (drawn AFTER
                // `paint_more_connections` has moved `cloning`).
                let cloning_arg = cloning
                    .as_ref()
                    .map(|(url_display, repo)| (url_display.to_string(), repo.to_string()));
                egui::CentralPanel::default().show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if ui.available_width() >= 1200.0 {
                            ui.columns(2, |columns| {
                                {
                                    let top_space =
                                        ((columns[0].available_height() - 730.0) * 0.5).max(16.0);
                                    paint_welcome_primary(
                                        &mut columns[0],
                                        self,
                                        top_space,
                                        cloning.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
                                    );
                                    columns[0].add_space(10.0);
                                }
                                paint_more_connections(
                                    &mut columns[0],
                                    self,
                                    &harness_setup_slot,
                                    &local_workspaces,
                                );
                                columns[1].vertical_centered(|ui| {
                                    welcome_setup_action =
                                        crate::ui::layout::paint_welcome_setup_board(
                                            ui,
                                            &self.machine_setup_tasks,
                                            &self.setup_attention,
                                            self.setup_probe.is_some(),
                                            &mut welcome_selection,
                                        );
                                });
                            });
                        } else {
                            paint_welcome_primary(
                                ui,
                                self,
                                ((ui.available_height() - 730.0) * 0.5).max(16.0),
                                cloning.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
                            );
                            ui.add_space(10.0);
                            paint_more_connections(
                                ui,
                                self,
                                &harness_setup_slot,
                                &local_workspaces,
                            );
                            ui.vertical_centered(|ui| {
                                welcome_setup_action = crate::ui::layout::paint_welcome_setup_board(
                                    ui,
                                    &self.machine_setup_tasks,
                                    &self.setup_attention,
                                    self.setup_probe.is_some(),
                                    &mut welcome_selection,
                                );
                            });
                        }
                    });
                    if self.wiz.is_some() {
                        crate::app::root::welcome_wizard::paint_wizard_modal(
                            ui,
                            self,
                            cloning_arg
                                .as_ref()
                                .map(|(url_display, repo)| (url_display.as_str(), repo.as_str())),
                        );
                    }
                });
                if self.wiz.is_none() && *harness_setup_slot.borrow() {
                    self.dialog = Some(Dialog::HarnessSetup(app_dialogs::DlgHarnessSetup::new()));
                }
            }
            Screen::Connected(_) => {
                let surface: &mut dyn Surface = self;
                crate::ui::layout::paint(ui, surface);
            }
        }

        if welcome_screen && matches!(&self.screen, Screen::Welcome) {
            if welcome_setup_action.retry {
                self.dispatch_ui_command(crate::ui::ApplicationCommand::RetrySetupCheck);
            }
            if let Some(task_id) = welcome_setup_action.fix_task {
                self.dispatch_ui_command(crate::ui::ApplicationCommand::RequestSetupTaskFix {
                    task_id,
                });
            }
            if let Some(selection) = welcome_selection.as_deref() {
                let tasks = self.machine_setup_tasks.clone();
                let notices = self.setup_attention.clone();
                let checking = self.setup_probe.is_some();
                let surface: &mut dyn Surface = self;
                if crate::ui::layout::paint_welcome_setup_details(
                    ui, surface, &tasks, &notices, checking, selection,
                ) {
                    welcome_selection = None;
                }
            }
            ui.ctx().data_mut(|data| {
                if let Some(selection) = welcome_selection {
                    data.insert_temp(selection_id, selection);
                } else {
                    data.remove::<String>(selection_id);
                }
            });
        } else {
            ui.ctx()
                .data_mut(|data| data.remove::<String>(selection_id));
        }

        self.render_queued_settings_dialog(ui);

        self.toasts.show(ui.ctx());
    }
}

impl KooladeApp {
    /// Route Workspace settings button clicks through the same dialog path in
    /// the live app and the egui click-path regression harness.
    pub(super) fn render_queued_settings_dialog(&mut self, ui: &mut egui::Ui) {
        let (open_project_settings, open_coding_settings) = ui.ctx().data_mut(|data| {
            let project_id = egui::Id::new("koolade_open_project_settings");
            let coding_id = egui::Id::new("koolade_open_coding_settings");
            let project = data.get_temp::<bool>(project_id).unwrap_or(false);
            let coding = data.get_temp::<bool>(coding_id).unwrap_or(false);
            data.remove_temp::<bool>(project_id);
            data.remove_temp::<bool>(coding_id);
            (project, coding)
        });
        if open_project_settings && let Screen::Connected(project) = &self.screen {
            self.dialog = Some(Dialog::ProjectSettings(
                app_dialogs::DlgProjectSettings::from_project(project),
            ));
        }
        if open_coding_settings {
            self.dialog = Some(Dialog::HarnessSetup(app_dialogs::DlgHarnessSetup::new()));
        }

        if let Some(dialog) = self.dialog.take() {
            self.render_dialog(ui, dialog);
        }
    }
}
