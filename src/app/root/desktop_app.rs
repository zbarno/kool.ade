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

        match &mut self.screen {
            Screen::Welcome => {
                let slot = std::cell::RefCell::new(false);
                let browse_slot = std::cell::RefCell::new(false);
                let clone_slot = std::cell::RefCell::new(false);
                let create_workspace_slot = std::cell::RefCell::new(false);
                let join_workspace_slot = std::cell::RefCell::new(false);
                let open_local_workspace_slot = std::cell::RefCell::new(false);
                let harness_setup_slot = std::cell::RefCell::new(false);
                let machine_setup_retry_slot = std::cell::RefCell::new(false);
                let local_workspaces =
                    crate::core::managed_workspace::WorkspaceManager::from_state_root()
                        .list_local();
                let local_workspaces = local_workspaces.unwrap_or_default();
                if self.conn_workspace_id.is_none() {
                    self.conn_workspace_id = local_workspaces.first().map(|workspace| workspace.id);
                }
                // In-flight badge for the card: ("github.com/{o}/{r}", repo).
                let cloning = self
                    .clone_job
                    .as_ref()
                    .map(|j| (j.url_display.as_str(), j.repo.as_str()));
                let clone_busy = cloning.is_some();
                egui::CentralPanel::default().show(ui, |ui| {
                    let top_space = ((ui.available_height() - 730.0) * 0.5).max(16.0);
                    egui::ScrollArea::vertical().show(ui, |ui| {
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
                                egui::ComboBox::from_id_salt("welcome_local_workspace")
                                    .selected_text(
                                        self.conn_workspace_id
                                            .and_then(|id| {
                                                local_workspaces
                                                    .iter()
                                                    .find(|workspace| workspace.id == id)
                                            })
                                            .map(|workspace| {
                                                format!("{} · {}", workspace.title, workspace.id)
                                            })
                                            .unwrap_or_else(|| "Choose a workspace".into()),
                                    )
                                    .width(field_width)
                                    .show_ui(ui, |ui| {
                                        for workspace in &local_workspaces {
                                            ui.selectable_value(
                                                &mut self.conn_workspace_id,
                                                Some(workspace.id),
                                                format!("{} · {}", workspace.title, workspace.id),
                                            );
                                        }
                                    });
                                if ui
                                    .add_enabled(
                                        !clone_busy,
                                        egui::Button::new("Open local workspace"),
                                    )
                                    .clicked()
                                {
                                    *open_local_workspace_slot.borrow_mut() = true;
                                }
                            }
                            ui.add_space(6.0);
                            if self.setup_probe.is_some() {
                                ui.label(crate::ui::theme::helper_text("Checking machine setup…"));
                            }
                            if ui.link("Configure coding tools…").clicked() {
                                *harness_setup_slot.borrow_mut() = true;
                            }
                            if !self.machine_setup_tasks.is_empty() {
                                ui.collapsing("Machine setup tasks", |ui| {
                                    for task in &self.machine_setup_tasks {
                                        if task.state
                                            != crate::persistence::setup_tasks::State::Verified
                                        {
                                            ui.label(egui::RichText::new(&task.title).strong());
                                            ui.label(crate::ui::theme::helper_text(
                                                &task.next_step,
                                            ));
                                        }
                                    }
                                    if ui.button("Check machine again").clicked() {
                                        *machine_setup_retry_slot.borrow_mut() = true;
                                    }
                                });
                            }
                            // The first-run readiness task is machine-local:
                            // show it even if no planning repository exists.
                            if let Some(issue) = &self.setup_attention {
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new(issue.title).strong());
                                ui.label(crate::ui::theme::helper_text(&issue.issue));
                                ui.label(crate::ui::theme::metadata_text(issue.next_action));
                            }
                        });
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
                if *machine_setup_retry_slot.borrow() {
                    self.refresh_setup_attention(true);
                }
            }
            Screen::Connected(_) => {
                let surface: &mut dyn Surface = self;
                crate::ui::layout::paint(ui, surface);
            }
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
