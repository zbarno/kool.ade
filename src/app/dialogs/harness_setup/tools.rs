use super::{DlgHarnessSetup, ProbeView};
use crate::ui::theme;
use egui::{RichText, Ui};

pub(super) fn paint_tools(ui: &mut Ui, dialog: &mut DlgHarnessSetup) {
    ui.label(theme::section_heading("Available coding tools"));
    ui.label(theme::helper_text(
        "Choose a tool to inspect its availability, executable path, and setup guidance.",
    ));
    if matches!(dialog.probe_view, ProbeView::Pending) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Checking installed tools…");
        });
        return;
    }
    let entries = dialog
        .settings
        .discovered
        .iter()
        .map(|(id, harness)| (id.clone(), harness.clone()))
        .collect::<Vec<_>>();
    if entries.is_empty() {
        ui.label("No supported coding tools were detected.");
        return;
    }
    let selected_id = egui::Id::new("koolade_settings_selected_tool");
    let mut selected = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(selected_id))
        .filter(|id| entries.iter().any(|(entry, _)| entry == id))
        .or_else(|| {
            dialog
                .settings
                .default_harness
                .clone()
                .filter(|id| entries.iter().any(|(key, _)| key == id))
        })
        .or_else(|| entries.first().map(|(id, _)| id.clone()));
    if ui.available_width() < 580.0 {
        egui::ComboBox::from_id_salt("coding_tool_picker")
            .selected_text(selected.as_deref().unwrap_or("Select a tool"))
            .width(ui.available_width())
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_ui(ui, |ui| {
                for (id, _) in &entries {
                    ui.selectable_value(&mut selected, Some(id.clone()), id);
                }
            });
        ui.add_space(theme::spacing::M);
        if let Some(id) = selected.as_deref()
            && let Some((_, harness)) = entries.iter().find(|(tool, _)| tool == id)
        {
            paint_tool_details(ui, dialog, id, harness);
        }
    } else {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(155.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    for (id, harness) in &entries {
                        let label = if dialog.settings.default_harness.as_deref() == Some(id) {
                            format!("{id} · Default")
                        } else {
                            id.clone()
                        };
                        if ui
                            .selectable_label(selected.as_deref() == Some(id.as_str()), label)
                            .clicked()
                        {
                            selected = Some(id.clone());
                        }
                        if !harness.ready {
                            ui.label(theme::metadata_text("Unavailable"));
                        }
                    }
                },
            );
            ui.separator();
            ui.vertical(|ui| {
                if let Some(id) = selected.as_deref()
                    && let Some((_, harness)) = entries.iter().find(|(tool, _)| tool == id)
                {
                    paint_tool_details(ui, dialog, id, harness);
                }
            });
        });
    }
    ui.ctx().data_mut(|data| {
        if let Some(selected) = selected {
            data.insert_temp(selected_id, selected);
        }
    });
    if dialog.settings.default_harness.as_ref().is_some_and(|id| {
        !dialog.settings.discovered.get(id).is_some_and(|harness| {
            harness.ready
                && harness.implementation_available
                && crate::harness::implementation_route_available(id)
        })
    }) {
        ui.colored_label(
            theme::DANGER,
            "Your saved default cannot run repository work inside Kool.ad/e's Bubblewrap sandbox. Select a supported tool before starting work.",
        );
    }
    if ui.button("Rediscover tools").clicked() {
        dialog.refresh();
    }
}

fn paint_tool_details(
    ui: &mut Ui,
    dialog: &mut DlgHarnessSetup,
    id: &str,
    harness: &crate::persistence::harness_settings::DetectedHarness,
) {
    if id == "pi" {
        super::pi_guide::paint(ui, dialog);
    }
    ui.label(theme::page_title(id));
    let availability = if harness.configuration_required {
        "Configuration required"
    } else if harness.ready {
        "Available"
    } else {
        "Unavailable"
    };
    ui.label(RichText::new(availability).color(if harness.ready {
        theme::SUCCESS
    } else {
        theme::DANGER
    }));
    ui.label(theme::helper_text(format!("Status: {}", harness.status)));
    if harness.ready && !harness.implementation_available {
        ui.label(theme::helper_text(
            "Implementation is unavailable for this tool until Kool.ad/e can run it inside the application-owned Linux sandbox. Pi requires Bubblewrap.",
        ));
    }
    if let Some(version) = &harness.version {
        ui.label(theme::helper_text(format!("Version: {version}")));
    }
    let configured = dialog.settings.manual_executable_paths.get(id).cloned();
    let current_path = configured.as_deref().or(harness.executable.as_deref());
    ui.label("Current executable");
    ui.label(
        RichText::new(current_path.unwrap_or("not found"))
            .monospace()
            .size(11.0)
            .color(theme::TEXT_DIM),
    );
    ui.label(if configured.is_some() {
        "Source: Manual"
    } else {
        "Source: Auto-detected"
    });
    if ui
        .add_enabled(
            harness.ready
                && harness.implementation_available
                && crate::harness::implementation_route_available(id),
            egui::RadioButton::new(
                dialog.settings.default_harness.as_deref() == Some(id),
                "Default tool",
            ),
        )
        .clicked()
    {
        dialog.select_default(id);
    }
    ui.add_space(theme::spacing::S);
    ui.label(RichText::new("Executable path").strong());
    let mut draft = dialog
        .manual_path_drafts
        .get(id)
        .cloned()
        .unwrap_or_else(|| {
            configured
                .clone()
                .or_else(|| harness.executable.clone())
                .unwrap_or_default()
        });
    ui.add(
        egui::TextEdit::singleline(&mut draft)
            .desired_width(f32::INFINITY)
            .hint_text("/path/to/cli"),
    );
    dialog
        .manual_path_drafts
        .insert(id.to_owned(), draft.clone());
    ui.horizontal_wrapped(|ui| {
        if ui.button("Use Path").clicked() {
            dialog.set_manual_path(id, draft.clone());
        }
        if configured.is_some() && ui.button("Return to auto-detect").clicked() {
            dialog.reset_manual_path(id);
        }
    });
    if let Some(diagnostic) = &harness.diagnostic {
        ui.colored_label(theme::DANGER, diagnostic);
    }
}
