use crate::ui::{Surface, task_detail};

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    ticket: &str,
    view: &task_detail::ViewModel,
) {
    ui.group(|ui| {
        ui.label(egui::RichText::new("Execution target").strong());
        let mut selected = view.target_preference.clone();
        let selected_label = view
            .target_choices
            .iter()
            .find(|choice| choice.preference == selected)
            .map(|choice| choice.label.as_str())
            .unwrap_or("Unavailable target");
        egui::ComboBox::from_id_salt(("task_execution_target", ticket))
            .selected_text(selected_label)
            .show_ui(ui, |ui| {
                for choice in &view.target_choices {
                    let response = ui.add_enabled(
                        choice.available,
                        egui::Button::selectable(
                            selected == choice.preference,
                            &choice.label,
                        ),
                    );
                    if response.clicked() {
                        selected = choice.preference.clone();
                    }
                    response.on_hover_text(&choice.reason);
                }
            });
        if selected != view.target_preference {
            surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
                task_detail::Command::SetExecutionTarget {
                    ticket: ticket.to_owned(),
                    preference: selected,
                },
            ));
        }
        if let Some(choice) = view
            .target_choices
            .iter()
            .find(|choice| choice.preference == view.target_preference)
        {
            ui.label(egui::RichText::new(&choice.reason).small().weak());
        }
        ui.label(egui::RichText::new("A preference does not grant execution permission. Unavailable or unapproved targets block execution; Kool.ad/e will not silently select another target or run on the host.").small().weak());
    });
}

pub(super) fn paint_execution_evidence(ui: &mut egui::Ui, view: &task_detail::ViewModel) {
    ui.group(|ui| {
        ui.label(egui::RichText::new("Latest execution record on this device").strong());
        if let Some(error) = &view.execution_evidence_error {
            ui.colored_label(
                crate::ui::theme::DANGER,
                format!("Execution evidence unavailable: {error}"),
            );
            return;
        }
        let Some(record) = &view.execution_evidence else {
            ui.label(
                "No persisted Docker execution record is available in this device's local state.",
            );
            return;
        };
        let worker = record
            .controller_target_id
            .as_deref()
            .filter(|target| *target != "local")
            .map_or("Local Docker worker", |worker_id| worker_id);
        ui.label(format!("Worker: {worker}"));
        ui.add(egui::Label::new(format!("Image digest: {}", record.image_digest)).wrap());
        ui.label(format!("Execution state: {}", record_state(record.state)));
        paint_execution_telemetry(ui, &record.phase_timings, &record.docker_usage);
        ui.label("Toolchain version is not stored in the execution record.");
        ui.label("Workspace transfer byte counts and per-file progress are not stored; stage-in and stage-out durations are shown above.");
    });
}

fn paint_execution_telemetry(
    ui: &mut egui::Ui,
    phases: &crate::persistence::execution_records::ExecutionPhaseTimingsV1,
    metrics: &crate::persistence::execution_records::DockerResourceUsageV1,
) {
    ui.label(format!(
        "Image preparation: {}",
        duration(phases.image_preparation_ms)
    ));
    ui.label(format!(
        "Workspace stage in: {}",
        duration(phases.workspace_stage_in_ms)
    ));
    ui.label(format!(
        "Workspace stage out: {}",
        duration(phases.workspace_stage_out_ms)
    ));
    ui.label(format!(
        "Dependency cache staging: {}",
        duration(phases.dependency_cache_stage_ms)
    ));
    ui.label("Dependency broker timing is recorded per request in task activity.");
    ui.label(format!(
        "Runtime QA container command: {}",
        duration(phases.qa_ms)
    ));
    ui.label(format!(
        "Container cleanup: {}",
        duration(phases.cleanup_ms)
    ));
    if metrics.sample_count == 0 {
        ui.label("Docker CPU, memory, and process measurements: Not recorded");
    } else {
        ui.label(format!(
            "Container CPU since startup: {}",
            metric(metrics.cpu_time_ms, "ms")
        ));
        ui.label(format!(
            "Sampled peak container memory: {}",
            metric(metrics.sampled_peak_memory_bytes, "bytes")
        ));
        ui.label(format!(
            "Latest sampled process count: {}",
            metric(metrics.latest_process_count, "")
        ));
        ui.label("Container disk usage: Not recorded");
        ui.label(format!("Docker samples recorded: {}", metrics.sample_count));
    }
}

fn duration(milliseconds: Option<u64>) -> String {
    milliseconds.map_or_else(|| "Not recorded".into(), |value| format!("{value} ms"))
}

fn metric<T: std::fmt::Display>(value: Option<T>, unit: &str) -> String {
    value.map_or_else(|| "Not recorded".into(), |value| format!("{value} {unit}"))
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod tests;

fn record_state(
    state: crate::persistence::execution_records::ExecutionRecordStateV1,
) -> &'static str {
    use crate::persistence::execution_records::ExecutionRecordStateV1 as State;
    match state {
        State::Preparing => "Preparing",
        State::Running => "Running",
        State::WaitingForDependencyApproval => "Waiting for dependency approval",
        State::WaitingForRuntime => "Waiting for runtime",
        State::CollectingResults => "Collecting results",
        State::Succeeded => "Succeeded",
        State::Failed => "Failed",
        State::Cancelled => "Cancelled",
        State::Interrupted => "Interrupted",
    }
}
