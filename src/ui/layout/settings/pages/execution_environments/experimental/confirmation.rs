use super::{save, testable};
use crate::{
    execution::docker::{environment::DockerEnvironmentStatus, images::ImageLock},
    persistence::experimental_features::{ExperimentalFeature, ExperimentalFeatureSettingsV1},
    ui::theme,
};

#[derive(Clone, Copy)]
pub(super) enum Confirmation {
    Mode,
    AllTestable,
}

pub(super) fn set_confirmation_pending(ctx: &egui::Context, choice: Confirmation) {
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("experimental_confirmation_pending"),
            match choice {
                Confirmation::Mode => 1_u8,
                Confirmation::AllTestable => 2_u8,
            },
        )
    });
}

pub(super) fn confirmation(
    ui: &mut egui::Ui,
    settings: &mut ExperimentalFeatureSettingsV1,
    docker: &DockerEnvironmentStatus,
    lock: Option<&ImageLock>,
    reviewer_is_ready: bool,
) {
    let choice = ui
        .ctx()
        .data(|data| data.get_temp::<u8>(egui::Id::new("experimental_confirmation_pending")));
    let Some(choice) = choice else { return };
    let choice = if choice == 2 {
        Confirmation::AllTestable
    } else {
        Confirmation::Mode
    };
    let mut open = true;
    egui::Window::new("Confirm experimental mode")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ui.ctx(), |ui| {
            ui.colored_label(theme::WARNING, "Experimental QA can exercise unfinished software. It remains isolated, but its results are not release certification.");
            ui.label("This confirmation is stored only in this user's application data.");
            if ui.button("I understand — enable on this machine").clicked() {
                settings.enabled = true;
                settings.confirmed_at_unix_ms = Some(crate::persistence::harness_settings::current_unix_ms().max(1));
                if matches!(choice, Confirmation::AllTestable) {
                    for (feature, profile) in [
                        (ExperimentalFeature::BrowserQa, "browser_qa"),
                        (ExperimentalFeature::LinuxDesktopQa, "desktop_qa"),
                    ] {
                        if testable(profile, docker, lock, reviewer_is_ready) {
                            settings.enabled_features.insert(feature);
                        }
                    }
                }
                save(ui, settings);
                ui.ctx().data_mut(|data| data.remove::<u8>(egui::Id::new("experimental_confirmation_pending")));
            }
            if ui.button("Cancel").clicked() {
                ui.ctx().data_mut(|data| data.remove::<u8>(egui::Id::new("experimental_confirmation_pending")));
            }
        });
    if !open {
        ui.ctx()
            .data_mut(|data| data.remove::<u8>(egui::Id::new("experimental_confirmation_pending")));
    }
}
