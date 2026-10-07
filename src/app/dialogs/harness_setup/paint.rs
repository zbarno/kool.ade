use super::{DlgHarnessSetup, HarnessSettingsSection, ProbeView};
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
        HarnessSettingsSection::Tools => paint_tools(ui, dialog),
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

fn paint_tools(ui: &mut egui::Ui, dialog: &mut DlgHarnessSetup) {
    ui.label(
        RichText::new("Available coding tools")
            .size(15.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.label(RichText::new("Kool.ad/e checks supported command line tools and keeps your default on this device. Configured means local provider settings were found; no live request was sent.").size(11.5).weak());
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
            let configured = dialog.settings.manual_executable_paths.get(&id).cloned();
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
                ui.label(format!("Status: {}", harness.status));
                if let Some(version) = &harness.version {
                    ui.label(format!("Version: {version}"));
                }
                let path = configured.as_deref().or(harness.executable.as_deref());
                ui.label(
                    RichText::new(format!("Executable: {}", path.unwrap_or("not found")))
                        .monospace()
                        .size(10.5)
                        .weak(),
                );
                ui.label(if configured.is_some() {
                    "Source: Manually configured"
                } else {
                    "Source: Auto-detected"
                });
                let mut draft = dialog
                    .manual_path_drafts
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| {
                        configured
                            .clone()
                            .or_else(|| harness.executable.clone())
                            .unwrap_or_default()
                    });
                let mut save_path = false;
                let mut reset_path = false;
                ui.horizontal(|ui| {
                    ui.label("Executable path");
                    ui.add(
                        egui::TextEdit::singleline(&mut draft)
                            .desired_width(300.0)
                            .hint_text("/path/to/cli"),
                    );
                    save_path = ui.button("Use Path").clicked();
                    reset_path =
                        configured.is_some() && ui.button("Use Auto-Detected Path").clicked();
                });
                dialog.manual_path_drafts.insert(id.clone(), draft.clone());
                if save_path {
                    dialog.set_manual_path(&id, draft);
                }
                if reset_path {
                    dialog.reset_manual_path(&id);
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
    super::pi_guide::paint(ui, dialog);
    ui.add_space(6.0);
    if ui.button("Rediscover tools").clicked() {
        dialog.refresh();
    }
}

fn paint_work_routes(ui: &mut egui::Ui, dialog: &mut DlgHarnessSetup) {
    ui.label(
        RichText::new("Models & routing")
            .size(15.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.label(RichText::new("Routes are saved on this device. Categories without an override use the application default: saved global CLI, legacy Codex setting, then Pi. An empty model uses the selected CLI's configured default.").size(11.0).weak());
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
    let available = dialog
        .settings
        .discovered
        .iter()
        .filter(|(_, harness)| harness.ready)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    for (key, label) in categories {
        let current = dialog
            .settings
            .work_routes
            .get(key)
            .map(|route| route.harness.clone());
        let mut selected = current.clone();
        ui.horizontal(|ui| {
            ui.label(label);
            egui::ComboBox::from_id_salt(("work-route", key))
                .selected_text(selected.as_deref().unwrap_or("Application default"))
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
                .is_some_and(|harness| harness.ready)
        {
            ui.label(
                RichText::new(format!("Saved {label} route uses {id}, which is currently unavailable. Kool.ad/e will surface the failure without switching tools."))
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
            ui.horizontal(|ui| {
                ui.label("Model");
                egui::ComboBox::from_id_salt(("work-model", key))
                    .selected_text(model.as_deref().unwrap_or(&default_label))
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
