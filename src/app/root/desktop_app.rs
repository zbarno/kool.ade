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
        ui.ctx().set_visuals(crate::ui::theme::koolade_visuals());
        ui.ctx().style_mut_of(egui::Theme::Dark, |style| {
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
            style
                .text_styles
                .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 7.0);
        });

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

        if let Some(dialog) = self.dialog.take() {
            self.render_dialog(ui, dialog);
        }

        self.toasts.show(ui.ctx());
    }
}
