use super::*;

/// "More connection options" column: the classic welcome card plus
/// workspace administration. A free function (not a closure) so the
/// welcome layout can compose it without closure-capture fights.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_more_connections(
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
            if welcome::paint_recent(ui, local_workspaces, &mut app.conn_workspace_id, clone_busy) {
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
}
/// The primary welcome card — permanently the journey doors; the ACTIVE
/// wizard renders as a modal OVER this canvas (see `paint_wizard_modal`),
/// shared by both layout breakpoints.
pub(super) fn paint_welcome_primary(
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
