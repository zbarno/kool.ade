use crate::{
    execution::contract::ResourceBudgetV1, persistence::remote_workers::RemoteWorkerRegistration,
};

pub(super) fn paint(ui: &mut egui::Ui, worker: &RemoteWorkerRegistration) {
    let ceiling = worker
        .last_probe
        .as_ref()
        .and_then(|probe| probe.resource_ceiling.as_ref());
    let Some(ceiling) = ceiling else {
        ui.label("Test this worker before setting its resource limit.");
        return;
    };
    let id = egui::Id::new(("remote_worker_resource_limit", worker.worker_id.as_str()));
    let base = worker
        .resource_limit
        .clone()
        .unwrap_or_else(|| ceiling.clone());
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.get_temp::<ResourceBudgetV1>(id).unwrap_or(base));
    draft.cpu_millis = draft.cpu_millis.min(ceiling.cpu_millis);
    draft.memory_bytes = draft.memory_bytes.min(ceiling.memory_bytes);
    draft.process_count = draft.process_count.min(ceiling.process_count);
    draft.temporary_disk_bytes = draft.temporary_disk_bytes.min(ceiling.temporary_disk_bytes);
    draft.output_bytes = draft.output_bytes.min(ceiling.output_bytes);
    let mut save = false;
    let mut reset = false;
    ui.collapsing("Per-worker resource limit", |ui| {
        ui.add(egui::Slider::new(&mut draft.cpu_millis, 1..=ceiling.cpu_millis).text("CPU millicores"));
        ui.add(egui::Slider::new(&mut draft.memory_bytes, 16 * 1024 * 1024..=ceiling.memory_bytes).text("Memory bytes"));
        ui.add(egui::Slider::new(&mut draft.process_count, 1..=ceiling.process_count).text("Processes"));
        ui.add(egui::Slider::new(&mut draft.temporary_disk_bytes, 16 * 1024 * 1024..=ceiling.temporary_disk_bytes).text("Task disk bytes"));
        ui.add(egui::Slider::new(&mut draft.output_bytes, 1024..=ceiling.output_bytes).text("Output bytes"));
        ui.label("Each execution is capped by both this limit and the worker's live Docker capabilities.");
        save = ui.button("Save resource limit").clicked();
        if worker.resource_limit.is_some() && ui.button("Use worker capability maximum").clicked() {
            match crate::persistence::remote_workers::configure_resource_limit(&worker.worker_id, None) {
                Ok(_) => reset = true,
                Err(error) => { ui.colored_label(crate::ui::theme::DANGER, error.to_string()); }
            }
        }
    });
    if reset {
        ui.ctx().data_mut(|data| {
            data.remove_temp::<ResourceBudgetV1>(id);
        });
        return;
    }
    ui.ctx()
        .data_mut(|data| data.insert_temp(id, draft.clone()));
    if save {
        match crate::persistence::remote_workers::configure_resource_limit(
            &worker.worker_id,
            Some(draft.clone()),
        ) {
            Ok(_) => ui.label("Saved per-worker resource ceiling."),
            Err(error) => ui.colored_label(
                crate::ui::theme::DANGER,
                format!("Could not save resource limit: {error}"),
            ),
        };
    }
}
