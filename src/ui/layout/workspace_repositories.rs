use super::HeaderAction;
use crate::ui::{ApplicationCommand, Surface};

pub(super) fn paint_menu(ui: &mut egui::Ui, surface: &mut dyn Surface) {
    ui.menu_button("Registered repositories", |ui| {
        let repositories = surface.registered_repositories();
        if repositories.is_empty() {
            ui.label("No registered repositories");
            return;
        }
        for repository in repositories {
            if ui
                .button(format!("Open {} in a new window", repository.label))
                .clicked()
            {
                surface.dispatch(ApplicationCommand::HeaderAction(
                    HeaderAction::OpenRegisteredRepository { id: repository.id },
                ));
                ui.close();
            }
        }
    });
}
