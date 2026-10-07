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
        .or_else(|| entries.first().map(|(id, _)| id.clone()));
    ui.columns(2, |columns| {
        columns[0].set_min_width(150.0);
        egui::ScrollArea::vertical()
            .id_salt("settings_tool_list")
            .show(&mut columns[0], |ui| {
                for (id, harness) in &entries {
                    let is_default = dialog.settings.default_harness.as_deref() == Some(id);
                    let label = if is_default {
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
                        ui.label(RichText::new("Unavailable").size(10.5).weak());
                    }
                }
            });
        if let Some(id) = selected.as_deref()
            && let Some((_, harness)) = entries.iter().find(|(tool, _)| tool == id)
        {
            paint_tool_details(&mut columns[1], dialog, id, harness);
        }
    });
    ui.ctx().data_mut(|data| {
        if let Some(selected) = selected {
            data.insert_temp(selected_id, selected);
        }
    });
    if dialog
        .settings
        .default_harness
        .as_ref()
        .is_some_and(|id| !dialog.settings.discovered.get(id).is_some_and(|h| h.ready))
    {
        ui.colored_label(
            theme::DANGER,
            "Your saved default is currently unavailable. Kool.ad/e will not silently select another tool.",
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
    super::pi_guide::paint(ui, dialog);
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
            harness.ready,
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
    ui.horizontal(|ui| {
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
