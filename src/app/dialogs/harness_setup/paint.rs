use super::{DlgHarnessSetup, ProbeView};
use crate::ui::theme;
use egui::RichText;

pub fn paint_harness_setup_card(ui: &mut egui::Ui, dialog: &mut DlgHarnessSetup) -> (bool, bool) {
    dialog.drain_probe();
    ui.label(
        RichText::new("Coding tools")
            .size(15.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.label(
        RichText::new(
            "Kool.ad/e checks supported command line tools and keeps your choice on this device. Configured means local provider settings were found; no live request was sent.",
        )
        .size(11.5)
        .weak(),
    );
    ui.add_space(8.0);
    if matches!(dialog.probe_view, ProbeView::Pending) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Checking installed tools…");
        });
    } else {
        let entries = dialog
            .settings
            .discovered
            .iter()
            .map(|(id, harness)| (id.clone(), harness.clone()))
            .collect::<Vec<_>>();
        for (id, harness) in entries {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&id).strong());
                    let readiness = if harness.configuration_required {
                        "Configuration required"
                    } else if harness.ready {
                        "Configured"
                    } else {
                        "Unavailable"
                    };
                    ui.label(RichText::new(readiness).color(if harness.ready {
                        theme::SUCCESS
                    } else {
                        theme::DANGER
                    }));
                    let selected = dialog.settings.default_harness.as_deref() == Some(&id);
                    if ui
                        .add_enabled(harness.ready, egui::RadioButton::new(selected, "Default"))
                        .clicked()
                    {
                        dialog.select_default(&id);
                    }
                });
                if let Some(path) = &harness.executable {
                    ui.label(RichText::new(path).monospace().size(10.5).weak());
                }
                if let Some(diagnostic) = &harness.diagnostic {
                    ui.label(RichText::new(diagnostic).size(11.0).color(theme::DANGER));
                }
            });
        }
    }
    if dialog
        .settings
        .default_harness
        .as_ref()
        .is_some_and(|id| !dialog.settings.discovered.get(id).is_some_and(|h| h.ready))
    {
        ui.label(
            RichText::new("Your saved default is currently unavailable. Kool.ad/e will not silently select another tool.")
                .size(11.0)
                .color(theme::DANGER),
        );
    }
    ui.add_space(6.0);
    if ui.button("Check again").clicked() {
        dialog.refresh();
    }
    if let Some((ok, message)) = &dialog.feedback {
        ui.label(RichText::new(message).size(11.0).color(if *ok {
            theme::SUCCESS
        } else {
            theme::DANGER
        }));
    }
    let mut close = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(RichText::new("Close").weak()).clicked() {
            close = true;
        }
    });
    (false, close)
}
