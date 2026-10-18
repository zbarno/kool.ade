use std::collections::HashSet;

use crate::ui::{Surface, task_detail};

mod constraints;

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
        constraints::paint(ui, surface, ticket, view);
        ui.collapsing("Required target capabilities", |ui| {
            let mut requirements = view.task_capabilities;
            let gpu_changed = ui.checkbox(&mut requirements.requires_gpu, "GPU access").changed();
            let display_changed = ui
                .checkbox(&mut requirements.requires_display, "Display access")
                .changed();
            if gpu_changed || display_changed {
                surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
                    task_detail::Command::SetTaskCapabilities {
                        ticket: ticket.to_owned(),
                        requirements,
                    },
                ));
            }
            ui.label(egui::RichText::new("Requirements are stored locally for this task. The current Docker targets do not expose GPU or display access, so a task requiring either is blocked instead of being routed to an incompatible target.").small().weak());
        });
        ui.label(egui::RichText::new("A preference does not grant execution permission. Unavailable or unapproved targets block execution; Kool.ad/e will not silently select another target or run on the host.").small().weak());
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Remote private-file grants").strong());
        ui.label(egui::RichText::new("Approval is scoped to the selected files, task workspace, and one remote worker, and expires after 15 minutes. Approved files are sent only when this task starts on that worker and are mounted read-only in its container.").small().weak());
        if let Some(error) = &view.remote_private_file_candidates_error {
            ui.colored_label(crate::ui::theme::DANGER, format!("Excluded file scan unavailable: {error}"));
        } else if view.remote_private_file_candidates_loading {
            ui.label(egui::RichText::new("Scanning excluded files…").small().weak());
        } else if !view.remote_private_file_candidates.is_empty() {
            ui.label(egui::RichText::new("Private files excluded from the remote workspace snapshot:").small());
            let selection_id = ui.make_persistent_id(("remote_private_file_selection", ticket));
            let mut selected = ui
                .ctx()
                .data(|data| data.get_temp::<HashSet<String>>(selection_id))
                .unwrap_or_default();
            for file in &view.remote_private_file_candidates {
                let mut checked = selected.contains(&file.path);
                let response = ui.checkbox(
                    &mut checked,
                    format!("{} · {:?} · {} bytes", file.path, file.data_class, file.size_bytes),
                );
                response.on_hover_text(format!("SHA-256: {}", file.sha256));
                if checked {
                    selected.insert(file.path.clone());
                } else {
                    selected.remove(&file.path);
                }
            }
            ui.ctx().data_mut(|data| data.insert_temp(selection_id, selected.clone()));
            if !view.remote_private_file_workers.is_empty() {
                let worker_id = ui.make_persistent_id(("remote_private_file_worker", ticket));
                let saved_worker = ui.ctx().data(|data| data.get_temp::<String>(worker_id));
                let default_worker = match &view.target_preference {
                    crate::execution::contract::TargetPreference::Worker(id)
                        if view.remote_private_file_workers.iter().any(|worker| worker.worker_id == *id) => Some(id.clone()),
                    _ => view.remote_private_file_workers.first().map(|worker| worker.worker_id.clone()),
                };
                let mut selected_worker = saved_worker
                    .filter(|id| view.remote_private_file_workers.iter().any(|worker| worker.worker_id == *id))
                    .or(default_worker)
                    .unwrap_or_default();
                let destination = view.remote_private_file_workers.iter()
                    .find(|worker| worker.worker_id == selected_worker)
                    .map(|worker| worker.destination.as_str())
                    .unwrap_or("Select a healthy enrolled worker");
                egui::ComboBox::from_id_salt(worker_id)
                    .selected_text(destination)
                    .show_ui(ui, |ui| {
                        for worker in &view.remote_private_file_workers {
                            ui.selectable_value(&mut selected_worker, worker.worker_id.clone(), &worker.destination);
                        }
                    });
                ui.ctx().data_mut(|data| data.insert_temp(worker_id, selected_worker.clone()));
                let mut paths = selected.into_iter().collect::<Vec<_>>();
                paths.sort();
                if ui.add_enabled(
                    !paths.is_empty() && !selected_worker.is_empty(),
                    egui::Button::new("Record local approval for selected files"),
                ).clicked() {
                    surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
                        task_detail::Command::GrantRemotePrivateFiles {
                            ticket: ticket.to_owned(),
                            worker_id: selected_worker,
                            paths,
                        },
                    ));
                }
            } else {
                ui.label(egui::RichText::new("Connect an enabled, healthy worker to record an approval.").small().weak());
            }
        } else if view.remote_private_file_candidates_error.is_none() && !view.remote_private_file_candidates_loading {
            ui.label(egui::RichText::new("No excluded private files were found in this task clone.").small().weak());
        }
        if let Some(error) = &view.remote_private_file_grants_error {
            ui.colored_label(crate::ui::theme::DANGER, format!("Grant state unavailable: {error}"));
        } else if view.remote_private_file_grants.is_empty() {
            ui.label(egui::RichText::new("No active grants for this task.").small().weak());
        } else {
            for grant in &view.remote_private_file_grants {
                ui.group(|ui| {
                    let remaining_minutes = grant.expires_at_unix_ms
                        .saturating_sub(crate::persistence::harness_settings::current_unix_ms())
                        .div_ceil(60_000);
                    ui.horizontal(|ui| {
                        ui.label(format!("Worker: {} · expires in about {remaining_minutes} min", grant.worker_id));
                        if ui.button("Revoke").on_hover_text("Remove this local approval receipt").clicked() {
                            surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
                                task_detail::Command::RevokeRemotePrivateFileGrant {
                                    ticket: ticket.to_owned(),
                                    grant_id: grant.grant_id.clone(),
                                },
                            ));
                        }
                    });
                    for (path, class) in &grant.files {
                        ui.label(format!("{path} · {class:?}"));
                    }
                });
            }
        }
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
