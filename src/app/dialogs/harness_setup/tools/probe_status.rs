use super::super::DlgHarnessSetup;
use crate::ui::theme;
use egui::Ui;

pub(super) fn paint_scan_progress(ui: &mut Ui, dialog: &DlgHarnessSetup) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label("Checking installed tools; completed provider results appear below.");
    });
    if !dialog.running_providers.is_empty() {
        ui.label(theme::metadata_text(format!(
            "In progress: {}",
            dialog
                .running_providers
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
}

pub(super) fn paint_probe_status(ui: &mut Ui, dialog: &DlgHarnessSetup, id: &str) {
    if dialog.running_providers.contains(id) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("Checking {id}…"));
        });
    }
    let Some(result) = dialog.probe_results.get(id) else {
        return;
    };
    ui.label(theme::metadata_text(format!(
        "Last probe: {} ms",
        result.duration_ms
    )));
    let catalog_status = match result.model_catalog_status {
        crate::harness::discovery::ModelCatalogStatus::Fresh => "fresh",
        crate::harness::discovery::ModelCatalogStatus::Stale => "stale",
        crate::harness::discovery::ModelCatalogStatus::Unavailable => "unavailable",
        crate::harness::discovery::ModelCatalogStatus::NotChecked => "not checked",
    };
    ui.label(theme::metadata_text(format!(
        "Model catalog: {catalog_status}"
    )));
    if let Some(stage) = result.failure_stage {
        let code = result
            .diagnostic_code
            .map(|code| format!(" ({})", code.label()))
            .unwrap_or_default();
        ui.label(theme::metadata_text(format!(
            "Probe detail: {}{code}",
            stage.label()
        )));
    }
}
