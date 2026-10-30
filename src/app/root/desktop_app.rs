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
        let selection_id = egui::Id::new("koolade_welcome_setup_selection");
        let mut welcome_selection = ui
            .ctx()
            .data_mut(|data| data.get_temp::<String>(selection_id));
        let mut welcome_setup_action = crate::ui::layout::WelcomeSetupAction::default();

        match &mut self.screen {
            Screen::Welcome => {
                // Folder-picker selections belong to the card that RAISED the
                // picker: the code-repository card by default. The
                // planning-repository card's Browse… button flips
                // `conn_browse_target` in the same frame before the consumer
                // below dispatches, so this reset cannot shadow it.
                self.conn_browse_target = BrowseTarget::CodeRepo;
                let slot = std::cell::RefCell::new(false);
                let browse_slot = std::cell::RefCell::new(false);
                let clone_slot = std::cell::RefCell::new(false);
                let create_workspace_slot = std::cell::RefCell::new(false);
                let join_workspace_slot = std::cell::RefCell::new(false);
                let open_local_workspace_slot = std::cell::RefCell::new(false);
                let harness_setup_slot = std::cell::RefCell::new(false);
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
                                    &browse_slot,
                                    &clone_slot,
                                    &slot,
                                    &create_workspace_slot,
                                    &join_workspace_slot,
                                    &open_local_workspace_slot,
                                    &harness_setup_slot,
                                    &local_workspaces,
                                    cloning,
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
                                &browse_slot,
                                &clone_slot,
                                &slot,
                                &create_workspace_slot,
                                &join_workspace_slot,
                                &open_local_workspace_slot,
                                &harness_setup_slot,
                                &local_workspaces,
                                cloning,
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
                if self.wiz.is_none() {
                    // Veiled while the wizard modal is open (see the card-bus
                    // guard): legacy actions must not fire from clicks the
                    // backdrop is eating.
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
                        // One picker serves both welcome cards; the active
                        // target decides which field receives the selection.
                        self.pick_folder_for_active_target();
                    }
                }
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

/// "More connection options" column: the classic welcome card plus
/// workspace administration. A free function (not a closure) so the
/// welcome layout can compose it without closure-capture fights.
#[allow(clippy::too_many_arguments)]
fn paint_more_connections(
    ui: &mut egui::Ui,
    app: &mut crate::app::root::KooladeApp,
    browse_slot: &std::cell::RefCell<bool>,
    clone_slot: &std::cell::RefCell<bool>,
    slot: &std::cell::RefCell<bool>,
    create_workspace_slot: &std::cell::RefCell<bool>,
    join_workspace_slot: &std::cell::RefCell<bool>,
    open_local_workspace_slot: &std::cell::RefCell<bool>,
    harness_setup_slot: &std::cell::RefCell<bool>,
    local_workspaces: &[crate::core::managed_workspace::LocalWorkspaceSummary],
    cloning: Option<(String, String)>,
) {
    let clone_busy = cloning.is_some();
    let cloning_refs = cloning.as_ref().map(|(a, b)| (a.as_str(), b.as_str()));
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("More connection options").strong());
        ui.label(crate::ui::theme::helper_text(
            "Classic connect, cloning, joins, and workspace administration.",
        ));
        ui.add_space(6.0);
        *slot.borrow_mut() = welcome::paint(
            ui,
            &mut app.conn_path,
            &mut app.conn_github,
            app.conn_error.as_deref(),
            &mut browse_slot.borrow_mut(),
            &mut clone_slot.borrow_mut(),
            cloning_refs,
            None,
            None,
        );
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label("Create a workspace with its own local planning repository");
        let field_width = ui.available_width().min(340.0);
        ui.add_sized(
            egui::vec2(field_width, 34.0),
            egui::TextEdit::singleline(&mut app.conn_create_title).hint_text("Project name"),
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
            egui::TextEdit::singleline(&mut app.conn_join_remote)
                .hint_text("Planning Git remote URL or local checkout path"),
        );
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let browse = ui
                .add_enabled(!clone_busy, egui::Button::new("Browse…"))
                .clicked();
            if browse {
                // The SAME picker the code-repository card
                // uses, aimed at the planning-repository
                // field: teams can point at a shared
                // planning checkout anywhere on disk.
                app.conn_browse_target = BrowseTarget::PlanningRepo;
                *browse_slot.borrow_mut() = true;
            }
            if ui
                .add_enabled(!clone_busy, egui::Button::new("Join workspace"))
                .clicked()
            {
                *join_workspace_slot.borrow_mut() = true;
            }
        });
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
                    app.conn_workspace_id
                        .and_then(|id| local_workspaces.iter().find(|workspace| workspace.id == id))
                        .map(|workspace| format!("{} · {}", workspace.title, workspace.id))
                        .unwrap_or_else(|| "Choose a workspace".into()),
                )
                .width(field_width)
                .show_ui(ui, |ui| {
                    for workspace in local_workspaces {
                        ui.selectable_value(
                            &mut app.conn_workspace_id,
                            Some(workspace.id),
                            format!("{} · {}", workspace.title, workspace.id),
                        );
                    }
                });
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
}
/// The primary welcome card — permanently the journey doors; the ACTIVE
/// wizard renders as a modal OVER this canvas (see `paint_wizard_modal`),
/// shared by both layout breakpoints.
fn paint_welcome_primary(
    ui: &mut egui::Ui,
    app: &mut crate::app::root::KooladeApp,
    top_space: f32,
    cloning: Option<(&str, &str)>,
) {
    ui.vertical_centered(|ui| {
        ui.add_space(top_space);
        crate::ui::theme::card_frame()
            .corner_radius(14)
            .stroke(egui::Stroke::new(1.0, crate::ui::theme::BORDER_STRONG))
            .inner_margin(24)
            .show(ui, |ui| {
                let door_req = std::cell::Cell::new(None);
                let wiz_action =
                    std::cell::Cell::new(None::<crate::app::root::welcome_wizard::WizAction>);
                let browse_req = std::cell::Cell::new(None);
                let bus = crate::app::root::welcome_wizard::PrimaryBus {
                    door: &door_req,
                    action: &wiz_action,
                    browse: &browse_req,
                };
                crate::app::root::welcome_wizard::paint_primary_card(ui, app, &bus, cloning);
                if app.wiz.is_some() {
                    // Presentation-only while the modal is open: ONE physical
                    // click can geometrically touch a canvas control and the
                    // modal backdrop in the same frame, and letting the canvas
                    // actuate would double-process the gesture (reopen or
                    // replace the wizard, then the backdrop closes it).
                    return;
                }
                if let Some(entry) = door_req.take() {
                    app.wizard_open(entry);
                }
                if let Some(action) = wiz_action.take() {
                    app.wizard_actuate(action);
                }
                if let Some(which) = browse_req.take() {
                    app.browse_wizard_field(which);
                }
            });
    });
}
