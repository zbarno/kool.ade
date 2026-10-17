use crate::{
    execution::{contract::TargetPreference, docker::environment::DockerEnvironmentStatus},
    persistence::{execution_environment_settings::ExecutionEnvironmentSettings, remote_workers},
    ui::theme,
};
mod runtime_qa;
mod smoke;
mod target;
mod worker_management;
mod workers;

pub(super) fn paint(ui: &mut egui::Ui) -> Option<String> {
    ui.label(theme::page_title("Execution Environments"));
    ui.label(theme::helper_text(
        "Configure trusted local and remote execution targets. Provider model routing stays under Coding Tools.",
    ));
    ui.add_space(theme::spacing::M);
    let (smoke_pending, smoke_result, smoke_finished) = smoke::poll(ui.ctx());
    if smoke_finished {
        DockerEnvironmentStatus::invalidate();
        crate::harness::runtime_capabilities::RuntimeCapabilities::invalidate();
    }
    let refresh_id = egui::Id::new("execution_environment_refresh_requested");
    let refresh = ui
        .ctx()
        .data_mut(|data| data.remove_temp::<bool>(refresh_id).unwrap_or(false));
    let status = if refresh {
        let status = DockerEnvironmentStatus::refresh();
        crate::harness::runtime_capabilities::RuntimeCapabilities::invalidate();
        status
    } else {
        DockerEnvironmentStatus::detect()
    };
    let workers = remote_workers::list();
    let (mut settings, load_error) = crate::persistence::execution_environment_settings::load();
    paint_view(
        ui,
        &status,
        workers.as_deref().unwrap_or(&[]),
        workers.as_ref().err().map(ToString::to_string).as_deref(),
        &mut settings,
        smoke_pending,
        smoke_result.as_deref(),
    );
    if let Some(error) = load_error {
        ui.colored_label(theme::WARNING, error);
    }
    None
}

pub(super) fn paint_view(
    ui: &mut egui::Ui,
    status: &DockerEnvironmentStatus,
    workers: &[remote_workers::RemoteWorkerRegistration],
    worker_error: Option<&str>,
    settings: &mut ExecutionEnvironmentSettings,
    smoke_pending: bool,
    smoke_result: Option<&str>,
) {
    ui.label(theme::section_heading("Local Docker"));
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(state_label(status.state)).strong());
        if let Some(version) = &status.daemon_version {
            ui.label(format!("Engine {version}"));
        }
        if let Some(architecture) = &status.architecture {
            ui.label(format!("Linux {architecture}"));
        }
        if status.desktop_vm {
            ui.label("Docker Desktop Linux VM");
        } else if status.rootless == Some(true) {
            ui.label("Rootless Engine");
        }
    });
    ui.label(theme::helper_text(&status.message));
    ui.label(theme::helper_text(format!(
        "Resource controls: CPU quota {}, memory limit {}, PID limit {}.",
        yes_no(status.cpu_limits),
        yes_no(status.memory_limits),
        yes_no(status.pids_limit)
    )));
    if let Some(bytes) = status.disk_available_bytes {
        ui.label(theme::metadata_text(format!(
            "Available staging / engine storage: {} GiB",
            bytes / 1024 / 1024 / 1024
        )));
    }
    if status.available_images.is_empty() {
        ui.label(theme::metadata_text(
            "Approved runtime image digests: none verified",
        ));
    } else {
        ui.label(theme::metadata_text(format!(
            "Approved runtime images verified: {}",
            status.available_images.join(", ")
        )));
    }
    ui.horizontal(|ui| {
        if ui.button("Refresh readiness").clicked() {
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("execution_environment_refresh_requested"),
                    true,
                )
            });
        }
        let enabled = status.engine_ready && !smoke_pending;
        let test_button = if enabled {
            ui.add_enabled(enabled, egui::Button::new("Test environment"))
        } else {
            ui.add_enabled(enabled, egui::Button::new("Test environment"))
                .on_disabled_hover_text(&status.message)
        };
        if test_button.clicked()
            && let Err(error) = smoke::start(ui.ctx())
        {
            ui.colored_label(theme::DANGER, error);
        }
    });
    if smoke_pending {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Running the approved no-network smoke container…");
        });
    }
    if let Some(result) = smoke_result {
        ui.colored_label(
            if result.starts_with("Passed") {
                theme::SUCCESS
            } else {
                theme::DANGER
            },
            result,
        );
    }
    ui.label(theme::helper_text(platform_guidance()));

    ui.add_space(theme::spacing::L);
    runtime_qa::paint(ui);

    ui.add_space(theme::spacing::L);
    ui.label(theme::section_heading("Remote Workers"));
    if let Some(error) = worker_error {
        ui.colored_label(
            theme::WARNING,
            format!("Could not read enrolled remote workers: {error}"),
        );
    }
    match workers {
        [] => {
            ui.label("No remote workers are enrolled on this device.");
        }
        workers => {
            for worker in workers {
                workers::paint_card(ui, worker);
            }
        }
    }
    worker_management::paint(ui);

    ui.add_space(theme::spacing::L);
    ui.label(theme::section_heading("Execution Defaults"));
    let mut selected = settings.default_target.clone();
    egui::ComboBox::from_id_salt("execution_default_target")
        .selected_text(target::label(&selected, workers))
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut selected,
                TargetPreference::Auto,
                "Automatic (trusted only)",
            );
            ui.selectable_value(&mut selected, TargetPreference::Local, "Local Docker");
            for worker in workers {
                ui.selectable_value(
                    &mut selected,
                    TargetPreference::Worker(worker.worker_id.clone()),
                    format!("Remote worker: {}", worker.worker_id),
                );
            }
        });
    if selected != settings.default_target {
        settings.default_target = selected;
        if let Err(error) = crate::persistence::execution_environment_settings::save(settings) {
            ui.colored_label(
                theme::DANGER,
                format!("Could not save target preference: {error}"),
            );
        }
    }
    ui.label(theme::helper_text(target::status(
        &settings.default_target,
        status,
        workers,
    )));
    ui.label(theme::metadata_text(
        "This user-local default guides new execution sessions; task claim and workspace authorization still determine whether a target may be used.",
    ));

    ui.add_space(theme::spacing::L);
    ui.label(theme::section_heading("Security / Approvals"));
    ui.label(theme::helper_text(
        "Remote code transfer requires a trusted worker identity and task authorization. Private file grants remain separate. No SSH private key is stored in these settings.",
    ));
}

pub(super) fn state_label(
    state: crate::execution::docker::environment::DockerEnvironmentState,
) -> &'static str {
    use crate::execution::docker::environment::DockerEnvironmentState as State;
    match state {
        State::NotInstalled => "Not installed",
        State::NotRunning => "Not running",
        State::PermissionDenied => "Permission denied",
        State::RootfulLinuxRejected => "Rootful Linux rejected",
        State::DesktopVmRequired => "Docker Desktop VM required",
        State::MissingApprovedImage => "Missing approved image",
        State::CapabilitiesIncompatible => "Capabilities incompatible",
        State::UntrustedEndpoint => "Untrusted endpoint rejected",
        State::Ready => "Ready",
    }
}

fn platform_guidance() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS: install and start Docker Desktop with Linux containers. Kool.ad/e uses its local Desktop VM endpoint and does not run host installation commands."
    } else if cfg!(target_os = "windows") {
        "Windows: install and start Docker Desktop with the Linux backend (WSL2 where supported). Native Windows containers are unsupported; no host installation commands are run."
    } else {
        "Linux: use rootless Docker Engine with the current user's daemon socket and UID maps. Rootful Docker and docker-group access are rejected; configure the daemon using your distribution's trusted instructions."
    }
}

fn yes_no(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "available",
        Some(false) => "unavailable",
        None => "unknown",
    }
}
