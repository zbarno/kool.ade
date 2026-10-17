use super::*;

/// Drives the production Settings Rediscover button twice. The real egui click path proves that
/// refresh invalidates and persists discovery state, cancels the previous generation, and starts
/// the replacement; direct state-method tests cannot prove the visible control is wired correctly.
/// Only worker launch is stubbed, so there is no Docker, SSH, network, or host-CLI dependency; setup
/// cost is two short frames and settings writes.
#[test]
fn rediscover_button_double_click_cancels_the_first_scan_generation() {
    let state_dir = std::env::temp_dir().join(format!(
        "koolade-discovery-refresh-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&state_dir).unwrap();
    let settings_path = state_dir.join("harness-settings.json");
    let mut capability_snapshot = super::codex_catalog_snapshot();
    capability_snapshot.provider_id = "pi".into();
    let mut initial_settings = HarnessSettings {
        schema_version: 3,
        default_harness: Some("pi".into()),
        ..HarnessSettings::default()
    };
    initial_settings.discovered.insert(
        "pi".into(),
        crate::persistence::harness_settings::DetectedHarness {
            status: "available".into(),
            version: Some("1.2.3".into()),
            executable: Some("/synthetic/pi".into()),
            diagnostic: None,
            ready: true,
            models: vec!["synthetic-model".into()],
            default_model: Some("synthetic-model".into()),
            model_catalog_fresh: true,
            configuration_required: false,
            implementation_available: true,
            capability_snapshot: Some(capability_snapshot),
            checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
        },
    );
    crate::persistence::harness_settings::with_settings_path(settings_path.clone(), || {
        crate::persistence::harness_settings::save(&initial_settings).unwrap();
    });
    let mut dialog = DlgHarnessSetup {
        settings: initial_settings,
        probe_view: ProbeView::Complete,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: Default::default(),
        probe_rx: None,
        scan_generation: 0,
        scan_cancel: None,
        runtime_available: None,
        scan_results: Vec::new(),
        probe_results: Default::default(),
        running_providers: Default::default(),
    };
    let context = egui::Context::default();
    let frame = |dialog: &mut DlgHarnessSetup, events| {
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 100.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                crate::app::dialogs::harness_setup::tools::paint_rediscover_button(ui, dialog);
            },
        );
        output.textures_delta.clear();
        output
    };
    let button_center = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Rediscover tools" => {
                    Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
                }
                _ => None,
            })
            .expect("Rediscover tools button is visible")
    };
    let click = |dialog: &mut DlgHarnessSetup, center: egui::Pos2| {
        for pressed in [true, false] {
            let _ = frame(
                dialog,
                vec![
                    egui::Event::PointerMoved(center),
                    egui::Event::PointerButton {
                        pos: center,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    };

    let center = button_center(&frame(&mut dialog, vec![]));
    crate::persistence::harness_settings::with_settings_path(settings_path, || {
        crate::app::dialogs::harness_setup::discovery::test_hooks::with_worker_launch_stub(|| {
            click(&mut dialog, center);
            let first_generation = dialog.scan_generation;
            let first_cancel = dialog.scan_cancel.as_ref().unwrap().clone();
            assert_discovery_invalidated_and_persisted();
            click(&mut dialog, center);

            assert!(dialog.scan_generation > first_generation);
            assert!(first_cancel.load(std::sync::atomic::Ordering::Acquire));
            assert_eq!(dialog.probe_view, ProbeView::Pending);
            assert!(dialog.probe_rx.is_some());
            assert!(dialog.probe_results.is_empty());
            assert!(dialog.scan_results.is_empty());
            assert!(
                dialog
                    .scan_cancel
                    .as_ref()
                    .is_some_and(|cancel| !cancel.load(std::sync::atomic::Ordering::Acquire))
            );
            assert_discovery_invalidated_and_persisted();
            assert_eq!(
                crate::app::dialogs::harness_setup::discovery::test_hooks::launched_generations(),
                vec![first_generation, dialog.scan_generation]
            );
        });
    });
    std::fs::remove_dir_all(state_dir).unwrap();
}

fn assert_discovery_invalidated_and_persisted() {
    let settings = crate::persistence::harness_settings::load().0;
    let detected = &settings.discovered["pi"];
    assert_eq!(settings.default_harness.as_deref(), Some("pi"));
    assert_eq!(detected.checked_at_unix_ms, 0);
    assert!(!detected.implementation_available);
    assert!(detected.capability_snapshot.is_none());
}
