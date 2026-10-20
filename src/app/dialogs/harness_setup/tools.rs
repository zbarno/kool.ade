use super::{DlgHarnessSetup, ProbeView};
use crate::ui::theme;
use egui::Ui;

mod details;
mod probe_status;

pub(super) fn paint_tools(ui: &mut Ui, dialog: &mut DlgHarnessSetup) {
    ui.label(theme::section_heading("Available coding tools"));
    ui.label(theme::helper_text(
        "Choose a tool to inspect its availability, executable path, and setup guidance.",
    ));
    if matches!(dialog.probe_view, ProbeView::Pending) {
        probe_status::paint_scan_progress(ui, dialog);
        let waiting = super::probes::PROVIDERS
            .iter()
            .filter(|id| {
                dialog.settings.discovered.get(**id).is_none_or(|entry| {
                    !entry.is_fresh_at(crate::persistence::harness_settings::current_unix_ms())
                })
            })
            .copied()
            .collect::<Vec<_>>();
        if !waiting.is_empty() {
            ui.label(theme::metadata_text(format!(
                "Still checking: {}",
                waiting.join(", ")
            )));
        }
    }
    let entries = dialog
        .settings
        .discovered
        .iter()
        .map(|(id, harness)| (id.clone(), harness.clone()))
        .collect::<Vec<_>>();
    if entries.is_empty() {
        if matches!(dialog.probe_view, ProbeView::Pending) {
            for id in super::probes::PROVIDERS {
                ui.label(format!("{id}: checking…"));
            }
            return;
        }
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
            details::paint_tool_details(ui, dialog, id, harness);
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
                        let fresh = harness
                            .is_fresh_at(crate::persistence::harness_settings::current_unix_ms());
                        if !fresh {
                            ui.label(theme::metadata_text("Stale; refresh"));
                        } else if !super::has_current_provider_capabilities(
                            id,
                            harness,
                            crate::persistence::harness_settings::current_unix_ms(),
                        ) {
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
                    details::paint_tool_details(ui, dialog, id, harness);
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
            super::has_current_provider_capabilities(
                id,
                harness,
                crate::persistence::harness_settings::current_unix_ms(),
            ) && harness.implementation_available
                && crate::harness::implementation_route_available(id)
        })
    }) {
        ui.colored_label(
            theme::DANGER,
            "Your saved default has no current supported repository execution capability. Refresh Coding tools and select an available tool before starting work.",
        );
    }
    paint_rediscover_button(ui, dialog);
}

pub(super) fn paint_rediscover_button(ui: &mut Ui, dialog: &mut DlgHarnessSetup) {
    if ui.button("Rediscover tools").clicked() {
        dialog.refresh();
    }
}
