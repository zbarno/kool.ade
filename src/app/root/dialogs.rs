use super::*;

impl KooladeApp {
    /// Paint a modal with the dialog moved OUT of `self` (clean borrows);
    /// the dialog is put back unless the user closed or completed it.
    pub(super) fn render_dialog(&mut self, ui: &mut egui::Ui, dialog: Dialog) {
        match dialog {
            Dialog::Import(mut d) => {
                let save_slot = std::cell::RefCell::new(false);
                let close_slot = std::cell::RefCell::new(false);
                let closed = crate::ui::overlays::show_modal(
                    ui,
                    true,
                    "Import reference documents",
                    560.0,
                    |ui| {
                        let (save, close) = app_dialogs::paint_import_card(ui, &mut d);
                        *save_slot.borrow_mut() = save;
                        *close_slot.borrow_mut() = close;
                    },
                );
                if *save_slot.borrow() {
                    self.perform_import(&mut d);
                }
                let positive = d.feedback.as_ref().is_some_and(|(ok, _)| *ok);
                if !closed && !*close_slot.borrow() && !positive {
                    self.dialog = Some(Dialog::Import(d));
                }
            }
            Dialog::Settings(mut d) => {
                let save_slot = std::cell::RefCell::new(false);
                let close_slot = std::cell::RefCell::new(false);
                let closed = crate::ui::overlays::show_modal(
                    ui,
                    true,
                    "People & stakeholders",
                    660.0,
                    |ui| {
                        let (save, close) = app_dialogs::paint_settings_card(ui, &mut d);
                        *save_slot.borrow_mut() = save;
                        *close_slot.borrow_mut() = close;
                    },
                );
                if *save_slot.borrow() {
                    self.perform_settings(&mut d);
                }
                let positive = d.feedback.as_ref().is_some_and(|(ok, _)| *ok);
                if !closed && !*close_slot.borrow() && !positive {
                    self.dialog = Some(Dialog::Settings(d));
                }
            }
            Dialog::ProjectSettings(mut d) => {
                let save_slot = std::cell::RefCell::new(false);
                let close_slot = std::cell::RefCell::new(false);
                let mut completed = false;
                let closed = crate::ui::overlays::show_modal(
                    ui,
                    true,
                    "Project / Git settings",
                    620.0,
                    |ui| {
                        let (save, close) = app_dialogs::paint_project_settings_card(ui, &mut d);
                        *save_slot.borrow_mut() = save;
                        *close_slot.borrow_mut() = close;
                    },
                );
                if *save_slot.borrow() {
                    let result = match &mut self.screen {
                        Screen::Connected(project) => d.apply(project),
                        Screen::Welcome => return,
                    };
                    match result {
                        Ok(Some(sha)) => {
                            completed = true;
                            self.toasts.success(format!("Saved · checkpoint {sha}"));
                        }
                        Ok(None) => {
                            completed = true;
                            self.toasts.info("Project settings already in sync");
                        }
                        Err(error) => d.feedback = Some((false, error.detail())),
                    }
                }
                if !closed && !*close_slot.borrow() && !completed {
                    self.dialog = Some(Dialog::ProjectSettings(d));
                }
            }
            Dialog::HarnessSetup(mut d) => {
                let close_slot = std::cell::RefCell::new(false);
                let closed = crate::ui::overlays::show_modal(
                    ui,
                    true,
                    "Coding tool configuration",
                    620.0,
                    |ui| {
                        let (_, close) = app_dialogs::paint_harness_setup_card(ui, &mut d);
                        *close_slot.borrow_mut() = close;
                    },
                );
                if !closed && !*close_slot.borrow() {
                    self.dialog = Some(Dialog::HarnessSetup(d));
                }
            }
            Dialog::Mcp(mut d) => {
                let save_slot = std::cell::RefCell::new(false);
                let close_slot = std::cell::RefCell::new(false);
                let closed = crate::ui::overlays::show_modal(
                    ui,
                    true,
                    "MCP server configuration",
                    660.0,
                    |ui| {
                        let (save, close) = app_dialogs::paint_mcp_card(ui, &mut d);
                        *save_slot.borrow_mut() = save;
                        *close_slot.borrow_mut() = close;
                    },
                );
                if *save_slot.borrow() {
                    self.perform_mcp(&mut d);
                }
                // Keep-open is owned by `perform_mcp` via `d.keep_open`
                // (a malformed save must survive its own successful feedback).
                if !closed && !*close_slot.borrow() && d.keep_open {
                    self.dialog = Some(Dialog::Mcp(d));
                }
            }
            #[cfg(test)]
            Dialog::Browse(mut d) => {
                let choose_slot = std::cell::RefCell::new(false);
                let cancel_slot = std::cell::RefCell::new(false);
                let closed = crate::ui::overlays::show_modal(
                    ui,
                    true,
                    "Choose a workspace folder",
                    560.0,
                    |ui| {
                        let (choose, cancel) = app_dialogs::paint_browse_card(ui, &mut d);
                        *choose_slot.borrow_mut() = choose;
                        *cancel_slot.borrow_mut() = cancel;
                    },
                );
                // Choosing writes ONLY `conn_path` (an absolute canonical
                // path): no connect attempt, no toast, and `conn_error`
                // keeps describing the last ATTEMPTED connect. The Open
                // button / Enter via submit_connect remains the single
                // connect authority. The defensive Welcome guard mirrors
                // the arms that only push from that screen.
                if *choose_slot.borrow() && matches!(self.screen, Screen::Welcome) {
                    self.conn_path = d.selection().to_string_lossy().into_owned();
                }
                // Standard put-back: reopen unless closed (X/Escape),
                // cancelled, or positively completed (Choose).
                if !closed && !*cancel_slot.borrow() && !*choose_slot.borrow() {
                    self.dialog = Some(Dialog::Browse(d));
                }
            }
        }
    }

    fn perform_import(&mut self, d: &mut DlgImport) {
        let result = match &mut self.screen {
            Screen::Connected(p) => d.apply(p),
            _ => return,
        };
        match result {
            Ok(n) => {
                d.feedback = Some((true, format!("Imported {n} document(s).")));
                if n > 0 {
                    self.dialog = None;
                    self.toasts
                        .success(format!("Imported {n} reference doc(s)"));
                }
            }
            Err(e) => d.feedback = Some((false, e.detail())),
        }
    }

    fn perform_settings(&mut self, d: &mut DlgSettings) {
        let result = match &mut self.screen {
            Screen::Connected(p) => d.apply(p),
            _ => return,
        };
        match result {
            Ok(sha) => {
                if let Screen::Connected(project) = &self.screen {
                    (self.cached_user, self.synth) = Self::derive_caches(project);
                }
                self.dialog = None;
                self.toasts.success(format!("Saved · checkpoint {sha}"));
            }
            Err(e) => d.feedback = Some((false, e.detail())),
        }
    }

    /// F-18 outcome table (design-pinned):
    /// * Unchanged → close, neutral feedback, INFO toast (zero churn, NFR-9).
    /// * Write + well-formed → close, success toast with the 7-char SHA.
    /// * Write + MALFORMED → KEEP OPEN with the sticky orange warning; the
    ///   save DID land, so feedback stays positive (consumers sit outside
    ///   the planner — D-16 non-blocking).
    /// * Clear → close, success toast with the checkpoint SHA.
    /// * Err → the disk write may have landed; the checkpoint FAILED. Honest
    ///   accounting: explain that retrying will NOT re-create the commit.
    fn perform_mcp(&mut self, d: &mut DlgMcp) {
        let result = match &mut self.screen {
            Screen::Connected(p) => d.apply(p),
            _ => return,
        };
        // Keep the header's dirty indicator truthful after a disk effect.
        if let Screen::Connected(p) = &mut self.screen {
            p.refresh_git();
        }
        match result {
            Ok(rec) => {
                let sha = rec.short_sha.clone().unwrap_or_default();
                match rec.op {
                    crate::artifacts::mcp_io::McpSaveOp::Unchanged => {
                        d.keep_open = false;
                        d.feedback = Some((true, "Unchanged — no write, no checkpoint.".into()));
                        self.toasts.info("MCP configuration already in sync");
                    }
                    crate::artifacts::mcp_io::McpSaveOp::Write => match rec.malformed {
                        None => {
                            d.keep_open = false;
                            d.feedback = Some((true, format!("Saved · checkpoint {sha}")));
                            self.toasts
                                .success(format!("MCP servers saved · checkpoint {sha}"));
                        }
                        Some(parse_error) => {
                            d.keep_open = true;
                            d.warning = Some(format!(
                                "Not valid JSON: {parse_error} — saved anyway; \
                                 consumers sit outside the planner. Turns \
                                 advertise it verbatim until fixed."
                            ));
                            d.feedback = Some((
                                true,
                                format!("Saved · checkpoint {sha} — kept open, see warning"),
                            ));
                        }
                    },
                    crate::artifacts::mcp_io::McpSaveOp::Clear => {
                        d.keep_open = false;
                        d.feedback = Some((true, format!("Cleared · checkpoint {sha}")));
                        self.toasts
                            .success(format!("MCP servers cleared · checkpoint {sha}"));
                    }
                }
            }
            Err(e) => {
                d.feedback = Some((
                    false,
                    format!(
                        "Disk write may have landed; git checkpoint failed: {} — \
                         retrying Save won't re-create the commit (the file \
                         already matches). Review git state or commit in a later turn.",
                        e.detail()
                    ),
                ));
                // keep_open stays as initialized (true): honest red feedback,
                // operator stays in the card to react.
            }
        }
    }
}
