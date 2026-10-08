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
        self.prepare_selected_task_chat(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        self.paint_screen(ui);
    }
}

impl KooladeApp {
    /// Drive conversation lifecycle from the modal selection transition,
    /// not from egui's repeated paint pass. The controller remains responsible
    /// for creating the durable introduction (idempotently by task identity).
    fn prepare_selected_task_chat(&mut self, ctx: &egui::Context) {
        let selected = ctx.data_mut(|data| {
            data.get_temp::<String>(egui::Id::new("koolade_selected_task"))
                .or_else(|| {
                    data.get_temp::<String>(egui::Id::new("koolade_selected_planning"))
                })
        });
        let tracked = egui::Id::new("koolade_last_prepared_task_chat");
        let previous = ctx.data_mut(|data| data.get_temp::<String>(tracked));
        if selected == previous {
            return;
        }
        ctx.data_mut(|data| {
            if let Some(key) = selected.as_ref() {
                data.insert_temp(tracked, key.clone());
            } else {
                data.remove::<String>(tracked);
            }
        });
        if let Some(key) = selected {
            self.prepare_task_chat(&key);
            ctx.request_repaint();
        }
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
                let harness_setup_slot = std::cell::RefCell::new(false);
                // In-flight badge for the card: ("github.com/{o}/{r}", repo).
                let cloning = self
                    .clone_job
                    .as_ref()
                    .map(|j| (j.url_display.as_str(), j.repo.as_str()));
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
                            if ui.link("Configure coding tools…").clicked() {
                                *harness_setup_slot.borrow_mut() = true;
                            }
                        });
                    });
                });
                if *slot.borrow() {
                    self.submit_connect();
                } else if *clone_slot.borrow() {
                    self.begin_clone_from_field();
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

#[cfg(test)]
mod task_chat_selection_tests {
    use super::*;

    #[test]
    fn selection_state_tracks_task_modal_open_switch_and_close() {
        let ctx = egui::Context::default();
        let mut app = KooladeApp::default();
        let selected = egui::Id::new("koolade_selected_task");
        let tracked = egui::Id::new("koolade_last_prepared_task_chat");
        ctx.data_mut(|data| data.insert_temp(selected, "task/a.md".to_owned()));
        app.prepare_selected_task_chat(&ctx);
        assert_eq!(
            ctx.data_mut(|data| data.get_temp::<String>(tracked)),
            Some("task/a.md".into())
        );
        // Ordinary redraw/tick for the same selection must not re-initialize.
        app.prepare_selected_task_chat(&ctx);
        assert_eq!(
            ctx.data_mut(|data| data.get_temp::<String>(tracked)),
            Some("task/a.md".into())
        );
        ctx.data_mut(|data| data.insert_temp(selected, "task/b.md".to_owned()));
        app.prepare_selected_task_chat(&ctx);
        assert_eq!(
            ctx.data_mut(|data| data.get_temp::<String>(tracked)),
            Some("task/b.md".into())
        );
        ctx.data_mut(|data| data.remove::<String>(selected));
        app.prepare_selected_task_chat(&ctx);
        assert_eq!(ctx.data_mut(|data| data.get_temp::<String>(tracked)), None);
    }
}
