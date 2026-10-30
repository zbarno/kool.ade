use super::*;
use eframe::{App, Frame};

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
        crate::ui::theme::apply(ui.ctx());
        let welcome_screen = matches!(&self.screen, Screen::Welcome);
        if welcome_screen && self.paint_onboarding(ui) {
            return;
        }
        let selection_id = egui::Id::new("koolade_welcome_setup_selection");
        let mut welcome_selection = ui
            .ctx()
            .data_mut(|data| data.get_temp::<String>(selection_id));
        let mut welcome_setup_action = crate::ui::layout::WelcomeSetupAction::default();

        match &mut self.screen {
            Screen::Welcome => {
                let slot = std::cell::RefCell::new(false);
                let browse_slot = std::cell::RefCell::new(false);
                let clone_slot = std::cell::RefCell::new(false);
                let create_workspace_slot = std::cell::RefCell::new(false);
                let join_workspace_slot = std::cell::RefCell::new(false);
                let open_local_workspace_slot = std::cell::RefCell::new(false);
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
                    .map(|j| (j.url_display.as_str(), j.repo.as_str()));
                let clone_busy = cloning.is_some();
                let mut paint_workspace_actions = |ui: &mut egui::Ui| {
                    let top_space = ((ui.available_height() - 730.0) * 0.5).max(16.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(top_space);
                        crate::ui::theme::card_frame()
                            .corner_radius(14)
                            .stroke(egui::Stroke::new(1.0, crate::ui::theme::BORDER_STRONG))
                            .inner_margin(24)
                            .show(ui, |ui| {
                                *slot.borrow_mut() = welcome::paint(
                                    ui,
                                    &mut self.conn_path,
                                    &mut self.conn_github,
                                    self.conn_error.as_deref(),
                                    &mut browse_slot.borrow_mut(),
                                    &mut clone_slot.borrow_mut(),
                                    cloning,
                                    None,
                                    None,
                                );
                            });
                    });
                    ui.add_space(10.0);
                    ui.vertical_centered(|ui| {
                        ui.label("Create a workspace with its own local planning repository");
                        let field_width = ui.available_width().min(340.0);
                        ui.add_sized(
                            egui::vec2(field_width, 34.0),
                            egui::TextEdit::singleline(&mut self.conn_create_title)
                                .hint_text("Project name"),
                        );
                        if ui
                            .add_enabled(!clone_busy, egui::Button::new("Create workspace"))
                            .clicked()
                        {
                            *create_workspace_slot.borrow_mut() = true;
                        }
                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(8.0);
                        ui.label("Join an existing shared workspace");
                        ui.add_sized(
                            egui::vec2(field_width, 34.0),
                            egui::TextEdit::singleline(&mut self.conn_join_remote)
                                .hint_text("Planning Git remote URL or local checkout path"),
                        );
                        if ui
                            .add_enabled(!clone_busy, egui::Button::new("Join workspace"))
                            .clicked()
                        {
                            *join_workspace_slot.borrow_mut() = true;
                        }
                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(8.0);
                        ui.label("Open an existing local workspace");
                        if local_workspaces.is_empty() {
                            ui.label(crate::ui::theme::helper_text(
                                "No local workspace is registered yet.",
                            ));
                        } else {
                            if welcome::paint_recent(
                                ui,
                                &local_workspaces,
                                &mut self.conn_workspace_id,
                                clone_busy,
                            ) {
                                *open_local_workspace_slot.borrow_mut() = true;
                            }
                            if ui
                                .add_enabled(!clone_busy, egui::Button::new("Open local workspace"))
                                .clicked()
                            {
                                *open_local_workspace_slot.borrow_mut() = true;
                            }
                        }
                        ui.add_space(6.0);
                        if ui.link("Configure coding tools…").clicked() {
                            *harness_setup_slot.borrow_mut() = true;
                        }
                    });
                };
                egui::CentralPanel::default().show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if ui.available_width() >= 1200.0 {
                            ui.columns(2, |columns| {
                                paint_workspace_actions(&mut columns[0]);
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
                            paint_workspace_actions(ui);
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
                });
                if *slot.borrow() {
                    self.submit_connect();
                } else if *clone_slot.borrow() {
                    self.begin_clone_from_field();
                } else if *create_workspace_slot.borrow() {
                    self.submit_create_workspace();
                } else if *join_workspace_slot.borrow() {
                    self.submit_join_workspace();
                } else if *open_local_workspace_slot.borrow() {
                    self.submit_open_local_workspace();
                } else if *browse_slot.borrow() {
                    // Start the native folder chooser at the current field's
                    // directory, or its parent when the field is a file path.
                    let current = std::path::PathBuf::from(self.conn_path.trim());
                    let start = if current.is_dir() {
                        Some(current.clone())
                    } else {
                        current
                            .parent()
                            .filter(|parent| parent.is_dir())
                            .map(std::path::Path::to_path_buf)
                    }
                    .or_else(|| std::env::current_dir().ok());
                    let mut picker = rfd::FileDialog::new().set_title("Choose a workspace folder");
                    if let Some(start) = start {
                        picker = picker.set_directory(start);
                    }
                    // Cancel leaves the field untouched. Selecting a folder
                    // only fills the field; Open workspace remains the
                    // single place that attempts a connection.
                    if let Some(path) = picker.pick_folder() {
                        let path = path.canonicalize().unwrap_or(path);
                        self.conn_path = path.to_string_lossy().into_owned();
                    }
                }
                if *harness_setup_slot.borrow() {
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
