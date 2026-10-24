//! Explicitly approved, locked Docker image preparation from setup task UI.
//!
//! Reuses the existing trusted smoke flow. It never accepts an arbitrary image
//! name, shell command or model-provided operation. On macOS/Windows it stays
//! disabled until the Docker Desktop transport is independently certified.

use super::*;

fn permitted(
    status: &crate::execution::docker::environment::DockerEnvironmentStatus,
) -> bool {
    cfg!(target_os = "linux")
        && status.engine_ready
        && !status.desktop_vm
        && status.rootless == Some(true)
        && status.available_images.is_empty()
}

pub(super) fn paint(ui: &mut egui::Ui, surface: &mut dyn Surface) {
    use crate::execution::docker::environment::DockerEnvironmentStatus;

    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("Prepare the approved image"));
    ui.label(
        "This downloads the architecture-matched, digest-locked general \
        runtime image and runs a no-network, non-root isolation smoke test. \
        It does not access your repositories, run host shell commands, or \
        request administrator privileges. Docker registry traffic and local \
        image storage will be used.",
    );
    let (running, result, finished) =
        crate::ui::layout::settings::poll_setup_image_smoke(ui.ctx());
    if finished {
        DockerEnvironmentStatus::invalidate();
        crate::harness::runtime_capabilities::RuntimeCapabilities::invalidate();
        surface.dispatch(crate::ui::ApplicationCommand::RetrySetupCheck);
    }

    let status = DockerEnvironmentStatus::detect();
    if !permitted(&status) {
        ui.label(crate::ui::theme::helper_text(
            "Image setup is only available on a verified native Linux rootless \
            Docker Engine for now. macOS and Windows Desktop transports must \
            pass their platform security acceptance tests first.",
        ));
    }
    if ui
        .add_enabled(
            permitted(&status) && !running,
            egui::Button::new("I approve image download and isolation test"),
        )
        .clicked()
        && let Err(error) =
            crate::ui::layout::settings::start_setup_image_smoke(ui.ctx())
    {
        ui.colored_label(crate::ui::theme::DANGER, error);
    }
    if running {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Preparing the approved Docker image and testing isolation…");
        });
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    if let Some(result) = result {
        ui.colored_label(
            if result.starts_with("Passed:") {
                crate::ui::theme::SUCCESS
            } else {
                crate::ui::theme::DANGER
            },
            result,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::docker::environment::DockerEnvironmentState;

    fn status() -> crate::execution::docker::environment::DockerEnvironmentStatus {
        crate::execution::docker::environment::DockerEnvironmentStatus {
            state: DockerEnvironmentState::MissingApprovedImage,
            engine_ready: true,
            message: "No approved image".into(),
            desktop_vm: false,
            daemon_version: Some("29.9.0".into()),
            architecture: Some("amd64".into()),
            rootless: Some(true),
            cpu_limits: Some(true),
            memory_limits: Some(true),
            pids_limit: Some(true),
            available_images: Vec::new(),
            disk_available_bytes: Some(8 * 1024 * 1024 * 1024),
        }
    }

    #[test]
    fn rejects_rootful_engine_and_unverified_docker_desktop() {
        let mut engine = status();
        assert_eq!(permitted(&engine), cfg!(target_os = "linux"));
        engine.rootless = Some(false);
        assert!(!permitted(&engine));
        engine.rootless = Some(true);
        engine.desktop_vm = true;
        assert!(!permitted(&engine));
        engine.desktop_vm = false;
        engine.engine_ready = false;
        assert!(!permitted(&engine));
    }

    #[test]
    fn only_unprepared_locked_image_needs_a_download() {
        let mut engine = status();
        engine.available_images.push("general".into());
        assert!(!permitted(&engine));
    }
}
