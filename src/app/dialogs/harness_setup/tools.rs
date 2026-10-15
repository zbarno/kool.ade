use super::{DlgHarnessSetup, ProbeView};
use crate::ui::theme;
use egui::{RichText, Ui};

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
    let fresh = harness.is_fresh_at(crate::persistence::harness_settings::current_unix_ms());
    ui.label(theme::page_title(id));
    let capability_ready = super::has_current_provider_capabilities(
        id,
        harness,
        crate::persistence::harness_settings::current_unix_ms(),
    );
    let availability = if !fresh {
        if matches!(dialog.probe_view, ProbeView::Pending) {
            "Checking this tool"
        } else {
            "Discovery expired; refresh required"
        }
    } else if harness.capability_snapshot.is_none() {
        "Capability snapshot unavailable; refresh required"
    } else if harness.configuration_required {
        "Configuration required"
    } else if capability_ready {
        "Available"
    } else {
        "Unavailable"
    };
    ui.label(RichText::new(availability).color(if capability_ready {
        theme::SUCCESS
    } else {
        theme::DANGER
    }));
    if fresh && let Some(snapshot) = harness.capability_snapshot.as_ref() {
        use crate::execution::contract::ReadinessState;
        let state_label = |state| match state {
            ReadinessState::Ready => "ready",
            ReadinessState::Unavailable => "unavailable",
            ReadinessState::Unknown => "not checked",
        };
        ui.label(theme::metadata_text(format!(
            "Capabilities: CLI {}, authentication {}, model catalog {}, scriptable invocation {}",
            state_label(snapshot.cli_binary),
            state_label(snapshot.authentication),
            state_label(snapshot.model_catalog),
            state_label(snapshot.scriptable_invocation),
        )));
        ui.label(theme::metadata_text(format!(
            "Execution: Docker bridge {}, target {}",
            state_label(snapshot.docker_mcp_bridge),
            state_label(snapshot.execution_target),
        )));
    }
    ui.label(theme::helper_text(if fresh {
        format!("Status: {}", harness.status)
    } else {
        format!(
            "Last result: {}. Rediscover to refresh provider and model status.",
            harness.status
        )
    }));
    probe_status::paint_probe_status(ui, dialog, id);
    ui.label(theme::helper_text(format!(
        "Security boundary: {}",
        crate::harness::execution_security::summary_for_harness(id)
    )));
    if fresh && harness.ready && !harness.implementation_available {
        ui.label(theme::helper_text(
            "Implementation is unavailable until Kool.ad/e has a ready Docker execution session for this tool. The approved runtime image must be available.",
        ));
    }
    if let Some(version) = &harness.version {
        ui.label(theme::helper_text(format!("Version: {version}")));
    }
    if super::has_current_model_catalog(
        id,
        harness,
        crate::persistence::harness_settings::current_unix_ms(),
    ) && !harness.models.is_empty()
    {
        ui.label(RichText::new("Models reported by this CLI").strong());
        ui.label(harness.models.join(", "));
        if let Some(model) = &harness.default_model {
            ui.label(theme::metadata_text(format!("CLI default: {model}")));
        }
    } else {
        ui.label(theme::metadata_text(
            "This CLI model catalog is unavailable or stale; refresh discovery before selecting a model.",
        ));
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
            capability_ready
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
        if ui.button(format!("Retry {id} discovery")).clicked() {
            dialog.retry_provider(id);
        }
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
