use super::*;

#[test]
fn execution_environments_is_a_routable_settings_page() {
    assert_eq!(
        Page::from_key("Execution Environments"),
        Some(Page::ExecutionEnvironments)
    );
    assert!(
        Page::ALL
            .iter()
            .any(|(page, title)| *page == Page::ExecutionEnvironments
                && *title == "Execution Environments")
    );
}

#[test]
fn execution_environment_view_shows_readiness_and_safe_setup_guidance() {
    let status = crate::execution::docker::environment::DockerEnvironmentStatus {
        state: crate::execution::docker::environment::DockerEnvironmentState::RootfulLinuxRejected,
        engine_ready: false,
        message: "Native Linux requires rootless Docker".into(),
        desktop_vm: false,
        daemon_version: Some("28.0.0".into()),
        architecture: Some("amd64".into()),
        rootless: Some(false),
        cpu_limits: Some(true),
        memory_limits: Some(true),
        pids_limit: Some(true),
        available_images: vec![],
        disk_available_bytes: None,
    };
    let mut settings =
        crate::persistence::execution_environment_settings::ExecutionEnvironmentSettings::default();
    let mut output = egui::Context::default().run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 900.0),
            )),
            ..Default::default()
        },
        |ui| {
            execution_environments::paint_view(ui, &status, &[], None, &mut settings, false, None);
        },
    );
    output.textures_delta.clear();
    let text = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for expected in [
        "Local Docker",
        "Rootful Linux rejected",
        "Engine 28.0.0",
        "Remote Workers",
        "Add worker",
        "Execution Defaults",
        "Security / Approvals",
        "Test environment",
        "Runtime QA",
        "Unavailable — release gates pending (#171)",
        "QA execution stays disabled",
        "Browser QA image:",
        "Linux desktop QA image and software display:",
        "disabled by reviewed image policy",
        "not verified locally",
        "Preview port relay: unavailable; Docker is not ready",
        "Evidence storage:",
        "Per-run evidence is capped",
        "there is no automatic expiry yet",
    ] {
        assert!(
            text.iter().any(|line| line.contains(expected)),
            "missing {expected}"
        );
    }
    assert!(
        text.iter()
            .any(|line| line.contains("rootless Docker Engine"))
    );
    assert!(text.iter().any(|line| line.contains("task claim")));
}
