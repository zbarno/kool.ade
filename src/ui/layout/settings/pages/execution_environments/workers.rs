use crate::persistence::remote_workers::RemoteWorkerRegistration;

pub(super) fn paint_card(ui: &mut egui::Ui, worker: &RemoteWorkerRegistration) {
    let health = worker
        .last_probe
        .as_ref()
        .map(|probe| format!("{:?}", probe.health))
        .unwrap_or_else(|| "Not tested".into());
    ui.group(|ui| {
        ui.label(egui::RichText::new(&worker.worker_id).strong());
        ui.label(format!(
            "SSH identity: {}@{}",
            worker.ssh_principal, worker.ssh_host_alias
        ));
        ui.label(format!("Host key: {}", worker.host_key_fingerprint));
        ui.label(format!(
            "{} · max {} concurrent tasks",
            health, worker.max_jobs
        ));
        ui.label(format!(
            "Architectures: {}",
            display_list(&worker.supported_architectures)
        ));
        ui.label(format!(
            "Approved image digests: {}",
            display_list(&worker.trusted_image_digests)
        ));
        ui.label(format!(
            "Policy revision: {}",
            empty_label(&worker.policy_revision)
        ));
        if let Some(probe) = &worker.last_probe {
            ui.label(format!(
                "Last heartbeat: {} · {} active tasks",
                probe.observed_at_unix_ms, probe.running_jobs
            ));
            ui.label(format!(
                "Observed Docker: {} · policy: {}",
                probe.docker_version.as_deref().unwrap_or("unknown"),
                empty_label(&probe.policy_revision)
            ));
            ui.label(format!(
                "Worker-reported observations: GPU {} · display {}",
                capability_label(probe.gpu_available),
                capability_label(probe.display_available)
            ));
            ui.label(egui::RichText::new("These observations do not grant device access. GPU and display tasks remain unavailable until supported runtime, display-mode, and device-grant policies are defined.").small().weak());
            if let Some(ceiling) = &probe.resource_ceiling {
                ui.label(format!(
                    "Reported capacity: {} CPU millicores, {} GiB memory, {} processes, {} GiB task disk, {} GiB output; {} GiB free staging disk",
                    ceiling.cpu_millis,
                    ceiling.memory_bytes / 1024 / 1024 / 1024,
                    ceiling.process_count,
                    ceiling.temporary_disk_bytes / 1024 / 1024 / 1024,
                    ceiling.output_bytes / 1024 / 1024 / 1024,
                    probe.disk_available_bytes.unwrap_or_default() / 1024 / 1024 / 1024,
                ));
            }
            ui.label(format!(
                "Worker-reported image digests: {}",
                display_list(&probe.trusted_image_digests)
            ));
        }
        super::worker_management::paint_worker_actions(ui, worker);
        super::worker_management::paint_resource_limit(ui, worker);
        let mut enabled = worker.enabled;
        if ui
            .checkbox(&mut enabled, "Allow scheduling on this worker")
            .changed()
        {
            configure(worker, enabled, worker.max_jobs, ui);
        }
        let mut max_jobs = worker.max_jobs;
        if ui
            .add(egui::Slider::new(&mut max_jobs, 1..=256).text("Concurrent task limit"))
            .changed()
        {
            configure(worker, worker.enabled, max_jobs, ui);
        }
    });
}

fn configure(worker: &RemoteWorkerRegistration, enabled: bool, max_jobs: u32, ui: &mut egui::Ui) {
    if let Err(error) = crate::persistence::remote_workers::configure(
        &worker.worker_id,
        enabled,
        max_jobs,
        worker.supported_architectures.clone(),
        worker.trusted_image_digests.clone(),
    ) {
        ui.colored_label(
            crate::ui::theme::DANGER,
            format!("Worker update failed: {error}"),
        );
    }
}

fn display_list(values: &[String]) -> String {
    if values.is_empty() {
        "not reported".into()
    } else {
        values.join(", ")
    }
}

fn empty_label(value: &str) -> &str {
    if value.is_empty() {
        "not reported"
    } else {
        value
    }
}

fn capability_label(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "available",
        Some(false) => "unavailable",
        None => "not reported",
    }
}
