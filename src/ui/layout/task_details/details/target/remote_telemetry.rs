use crate::persistence::remote_execution_telemetry::RemoteExecutionTelemetryV1;

pub(super) fn paint(
    ui: &mut egui::Ui,
    telemetry: Option<&RemoteExecutionTelemetryV1>,
    error: Option<&str>,
) {
    ui.label(egui::RichText::new("Remote worker telemetry").strong());
    if let Some(error) = error {
        ui.colored_label(
            crate::ui::theme::DANGER,
            format!("Remote telemetry unavailable: {error}"),
        );
        return;
    }
    let Some(telemetry) = telemetry else {
        ui.label("Remote transfer telemetry was not recorded for this execution.");
        return;
    };
    ui.label(format!("Telemetry worker: {}", telemetry.worker_id));
    ui.label(format!(
        "Workspace upload: {} bytes in {} ms",
        metric(telemetry.workspace_upload_bytes),
        metric(telemetry.workspace_upload_ms)
    ));
    ui.label(format!(
        "Result download: {} bytes in {} ms",
        metric(telemetry.result_download_bytes),
        metric(telemetry.result_download_ms)
    ));
    ui.label(format!(
        "Remote execution wall time: {}",
        duration(telemetry.execution_wall_ms)
    ));
    ui.label(format!(
        "Remote CPU time: {} ms; sampled peak memory: {} bytes",
        metric(telemetry.execution_cpu_millis),
        metric(telemetry.execution_peak_memory_bytes)
    ));
    if let Some(digest) = &telemetry.image_digest {
        ui.add(egui::Label::new(format!("Remote image digest: {digest}")).wrap());
    }
}

fn metric(value: Option<u64>) -> String {
    value.map_or_else(|| "Not recorded".into(), |value| value.to_string())
}

fn duration(milliseconds: Option<u64>) -> String {
    milliseconds.map_or_else(|| "Not recorded".into(), |value| format!("{value} ms"))
}
