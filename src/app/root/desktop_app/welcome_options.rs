use super::*;

/// Local repository access and coding-tool setup.
pub(super) fn paint_more_connections(
    ui: &mut egui::Ui,
    app: &mut crate::app::root::KooladeApp,
    harness_setup_slot: &std::cell::RefCell<bool>,
) {
    ui.vertical_centered(|ui| {
        if ui.button("Open a local code repository…").clicked()
            && let Some(path) = rfd::FileDialog::new().pick_folder()
        {
            app.finish_connect(welcome::attempt_connect(&path.to_string_lossy()));
        }
        ui.add_space(6.0);
        if ui.link("Configure coding tools…").clicked() {
            *harness_setup_slot.borrow_mut() = true;
        }
    });
}

/// Main welcome card with the code-repository clone journey.
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
                let bus = crate::app::root::welcome_wizard::PrimaryBus {
                    door: &door,
                    action: &action,
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
            });
    });
}
