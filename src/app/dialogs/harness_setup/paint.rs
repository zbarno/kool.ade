use super::{DlgHarnessSetup, HarnessSettingsSection};
use crate::ui::theme;
use egui::RichText;

pub fn paint_harness_setup_card(ui: &mut egui::Ui, dialog: &mut DlgHarnessSetup) -> (bool, bool) {
    dialog.drain_probe();
    ui.horizontal(|ui| {
        section_tab(
            ui,
            &mut dialog.section,
            HarnessSettingsSection::Tools,
            "Coding tools",
        );
        section_tab(
            ui,
            &mut dialog.section,
            HarnessSettingsSection::Routing,
            "Models & routing",
        );
    });
    ui.separator();
    match dialog.section {
        HarnessSettingsSection::Tools => super::tools::paint_tools(ui, dialog),
        HarnessSettingsSection::Routing => paint_work_routes(ui, dialog),
    }
    ui.add_space(6.0);
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

fn section_tab(
    ui: &mut egui::Ui,
    selected: &mut HarnessSettingsSection,
    section: HarnessSettingsSection,
    label: &str,
) {
    let active = *selected == section;
    let text =
        RichText::new(label)
            .strong()
            .color(if active { theme::TEXT } else { theme::TEXT_DIM });
    let response = ui.add(egui::Button::new(text).selected(active));
    if response.clicked() {
        *selected = section;
    }
}

pub(super) use super::tools::paint_tools;

fn route_available(
    id: &str,
    harness: &crate::persistence::harness_settings::DetectedHarness,
) -> bool {
    harness.ready
        && harness.implementation_available
        && crate::harness::implementation_route_available(id)
}

pub(super) fn paint_work_routes(ui: &mut egui::Ui, dialog: &mut DlgHarnessSetup) {
    ui.label(
        RichText::new("Models & routing")
            .size(15.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.label(RichText::new("Choose the coding tool and model for each kind of work. Settings are saved on this device. Unchanged categories use your default tool.").size(11.0).weak());
    ui.label(RichText::new("Repository commands run through Kool.ad/e's application-managed Bubblewrap boundary. Supported provider CLIs keep their own model connection; their native repository tools are disabled.").size(10.5).weak());
    let categories = [
        (
            crate::persistence::harness_settings::IMPLEMENTATION,
            "Implementation",
        ),
        (
            crate::persistence::harness_settings::MANAGER,
            "Kool.ad/e Manager",
        ),
        (
            crate::persistence::harness_settings::QA,
            "QA / Verification",
        ),
        (
            crate::persistence::harness_settings::DOCUMENTATION,
            "Documentation",
        ),
    ];
    for (key, label) in categories {
        let available = dialog
            .settings
            .discovered
            .iter()
            .filter(|(id, harness)| route_available(id, harness))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let current = dialog
            .settings
            .work_routes
            .get(key)
            .map(|route| route.harness.clone());
        let mut selected = current.clone();
        ui.vertical(|ui| {
            ui.add_space(theme::spacing::M);
            ui.label(theme::section_heading(label));
            egui::ComboBox::from_id_salt(("work-route", key))
                .selected_text(selected.as_deref().unwrap_or("Application default"))
                .width(ui.available_width().min(360.0))
                .wrap_mode(egui::TextWrapMode::Truncate)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut selected, None, "Application default");
                    for id in &available {
                        ui.selectable_value(&mut selected, Some(id.clone()), id);
                    }
                });
        });
        if selected != current {
            if let Some(id) = selected {
                dialog.select_work_route(key, &id);
            } else {
                dialog.settings.work_routes.remove(key);
                dialog.persist();
            }
        }
        if let Some(id) = current.as_deref()
            && !dialog
                .settings
                .discovered
                .get(id)
                .is_some_and(|harness| route_available(id, harness))
        {
            let detail = if dialog
                .settings
                .discovered
                .get(id)
                .is_some_and(|harness| harness.ready)
            {
                "does not support repository work inside Kool.ad/e's application-owned sandbox"
            } else {
                "is currently unavailable"
            };
            ui.label(
                RichText::new(format!("Saved {label} route uses {id}, which {detail}. Choose an available route before starting work."))
                    .size(10.5)
                    .color(theme::DANGER),
            );
        }
        if let Some(route) = dialog.settings.work_routes.get(key) {
            let id = route.harness.clone();
            let current_model = route.model.clone();
            let models = dialog
                .settings
                .discovered
                .get(&id)
                .map(|detected| detected.models.clone())
                .unwrap_or_default();
            let default_model = dialog
                .settings
                .discovered
                .get(&id)
                .and_then(|detected| detected.default_model.as_deref());
            let default_label = default_model
                .map(|model| format!("CLI default (last discovered: {model})"))
                .unwrap_or_else(|| "CLI default (model not reported)".into());
            let mut model = current_model.clone();
            ui.vertical(|ui| {
                ui.label("Model");
                egui::ComboBox::from_id_salt(("work-model", key))
                    .selected_text(model.as_deref().unwrap_or(&default_label))
                    .width(ui.available_width().min(360.0))
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut model, None, default_label.as_str());
                        for candidate in &models {
                            ui.selectable_value(&mut model, Some(candidate.clone()), candidate);
                        }
                    });
            });
            if model != current_model {
                dialog.set_work_model(key, model.clone().unwrap_or_default());
            }
            if model.is_some()
                && !models
                    .iter()
                    .any(|candidate| Some(candidate) == model.as_ref())
            {
                ui.label(
                    RichText::new("The saved model is no longer present in this CLI's configured catalog; choose an available model or its CLI default.")
                        .size(10.5)
                        .color(theme::DANGER),
                );
            }
        }
    }
}
