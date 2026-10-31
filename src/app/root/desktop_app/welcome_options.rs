use super::*;

/// Returning-workspace access stays available without exposing arbitrary code
/// or planning checkout paths.
pub(super) fn paint_more_connections(
    ui: &mut egui::Ui,
    app: &mut crate::app::root::KooladeApp,
    harness_setup_slot: &std::cell::RefCell<bool>,
    local_workspaces: &[crate::core::managed_workspace::LocalWorkspaceSummary],
) {
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("Your managed workspaces").strong());
        ui.label(crate::ui::theme::helper_text(
            "Kool.ad/e stores planning data separately and opens workspaces from this list.",
        ));
        ui.add_space(6.0);
        if local_workspaces.is_empty() {
            ui.label(crate::ui::theme::helper_text(
                "No managed workspaces are registered on this computer yet.",
            ));
        } else if welcome::paint_recent(ui, local_workspaces, &mut app.conn_workspace_id, false) {
            app.submit_open_local_workspace();
        }
        ui.add_space(6.0);
        if ui.link("Configure coding tools…").clicked() {
            *harness_setup_slot.borrow_mut() = true;
        }
    });
}

/// Main welcome card with the clone and blank-project journeys.
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
                let door = std::cell::Cell::new(None);
                let action =
                    std::cell::Cell::new(None::<crate::app::root::welcome_wizard::WizAction>);
                let browse = std::cell::Cell::new(None);
                let bus = crate::app::root::welcome_wizard::PrimaryBus {
                    door: &door,
                    action: &action,
                    browse: &browse,
                };
                crate::app::root::welcome_wizard::paint_primary_card(ui, app, &bus, cloning);
                if app.wiz.is_some() {
                    return;
                }
                if let Some(entry) = door.take() {
                    app.wizard_open(entry);
                }
                if let Some(action) = action.take() {
                    app.wizard_actuate(action);
                }
                if let Some(which) = browse.take() {
                    app.browse_wizard_field(which);
                }
            });
    });
}
