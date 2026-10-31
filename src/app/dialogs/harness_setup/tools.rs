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
    }
    let now_ms = crate::persistence::harness_settings::current_unix_ms();
    // Normal tool selection is not an installation/repair catalog. Show only
    // providers verified for real work; the rest live under Add / Fix.
    let entries = dialog
        .settings
        .discovered
        .iter()
        .filter(|(id, harness)| harness.is_currently_actionable(id, now_ms))
        .map(|(id, harness)| (id.clone(), harness.clone()))
        .collect::<Vec<_>>();
    if entries.is_empty() {
        ui.label(if matches!(dialog.probe_view, ProbeView::Pending) {
            "Checking coding tools…"
        } else {
            "No ready coding tools. Use Add / Fix a coding tool below."
        });
    } else {
        // Exactly one actionable provider narrows the default choice to a
    // single click, so that row earns a visible marker. The predicate is
    // the same canonical one the router's task-detail notice uses.
    let ready_count = entries
        .iter()
        .filter(|(id, harness)| harness.is_currently_actionable(id, now_ms))
        .count();
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
        // Narrow picker branch: the suggested-default marker is wide-grid
        // only, deliberately. Below 580 px the row collapses to the combo
        // box + details stack and the marker gains no reach; operators on
        // such windows lean on the details-pane status instead.
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
                        let status = details::availability_label(
                            id,
                            harness,
                            crate::persistence::harness_settings::current_unix_ms(),
                            matches!(dialog.probe_view, ProbeView::Pending),
                        );
                        if status != "Available" {
                            ui.label(theme::metadata_text(status));
                        }
                        if ready_count == 1 && harness.is_currently_actionable(id, now_ms) {
                            ui.colored_label(theme::SUCCESS, "Ready \u{2014} suggested default");
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
    if let Some(default_id) = dialog.settings.default_harness.as_ref()
        && !dialog
            .settings
            .discovered
            .get(default_id)
            .is_some_and(|harness| {
                details::availability_label(
                    default_id,
                    harness,
                    crate::persistence::harness_settings::current_unix_ms(),
                    false,
                ) == "Available"
            })
    {
        let mut message =
            format!("Your saved default {default_id} is not verified for context-only planning.");
        if let Some(blocker) = dialog
            .settings
            .discovered
            .get(default_id)
            .and_then(|harness| harness.diagnostic.as_deref())
            .filter(|detail| !detail.trim().is_empty())
        {
            message.push(' ');
            message.push_str(blocker);
        }
        message.push_str(
            " Kool.ad/e checks installed tools automatically. If this persists, open Add / Fix a coding tool below. Repository reads still need Docker.",
        );
        ui.colored_label(theme::DANGER, message);
    }
    }
    // Installation and troubleshooting are deliberately separate from the
    // default/role selection list. Preserve access to manual paths and login.
    ui.collapsing("Add / Fix a coding tool", |ui| {
        for id in super::probes::PROVIDERS {
            if entries.iter().any(|(visible, _)| visible.as_str() == id) {
                continue;
            }
            ui.collapsing(id, |ui| {
                if let Some(harness) = dialog.settings.discovered.get(id).cloned() {
                    details::paint_tool_details(ui, dialog, id, &harness);
                } else {
                    ui.label("Not checked yet. Run discovery to check installation and sign-in.");
                    let provider = crate::app::setup_attention::catalog::ProviderIdV1::from_probe_id(id);
                    if let Some(provider) = provider {
                        ui.hyperlink_to("Installation guide", provider.installation_guide());
                    }
                    if ui.button(format!("Check {id}")).clicked() {
                        dialog.retry_provider(id);
                    }
                    ui.label("Custom executable path");
                    let draft = dialog.manual_path_drafts.entry(id.to_owned()).or_default();
                    ui.text_edit_singleline(draft);
                    if ui.button("Use path").clicked() {
                        let selected_path = dialog.manual_path_drafts.get(id).cloned().unwrap_or_default();
                        dialog.set_manual_path(id, selected_path);
                    }
                }
            });
        }
    });
    paint_rediscover_button(ui, dialog);
}

pub(super) fn paint_rediscover_button(ui: &mut Ui, dialog: &mut DlgHarnessSetup) {
    if ui.button("Rediscover tools").clicked() {
        dialog.refresh();
    }
}
