use super::*;

/// Production boundary: clicking one provider's Retry action invalidates and persists only that
/// provider, then starts a new generation. Direct method tests cannot prove the visible button
/// invokes it. Headless egui, a temporary settings file, and stubbed worker launch are used;
/// setup cost is one frame and a settings transaction.
#[test]
fn retry_button_rechecks_only_the_selected_provider() {
    use crate::persistence::harness_settings::DetectedHarness;

    let root = std::env::temp_dir().join(format!(
        "koolade-provider-retry-ui-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let settings_path = root.join("harness-settings.json");
    let now = crate::persistence::harness_settings::current_unix_ms();
    let snapshot = super::codex_catalog_snapshot();
    let mut pi_snapshot = snapshot.clone();
    pi_snapshot.provider_id = "pi".into();
    let mut settings = HarnessSettings {
        default_harness: Some("pi".into()),
        ..HarnessSettings::default()
    };
    settings.discovered.insert(
        "pi".into(),
        DetectedHarness {
            status: "available".into(),
            version: Some("1.2.3".into()),
            executable: Some("/synthetic/pi".into()),
            diagnostic: None,
            ready: true,
            models: vec!["synthetic-pi-model".into()],
            default_model: Some("synthetic-pi-model".into()),
            model_catalog_fresh: true,
            configuration_required: false,
            implementation_available: true,
            capability_snapshot: Some(pi_snapshot),
            checked_at_unix_ms: now,
        },
    );
    settings.discovered.insert(
        "codex".into(),
        DetectedHarness {
            status: "available".into(),
            version: Some("0.162.0".into()),
            executable: Some("/synthetic/codex".into()),
            diagnostic: None,
            ready: true,
            models: vec!["synthetic-codex-model".into()],
            default_model: Some("synthetic-codex-model".into()),
            model_catalog_fresh: true,
            configuration_required: false,
            implementation_available: true,
            capability_snapshot: Some(snapshot),
            checked_at_unix_ms: now,
        },
    );

    crate::persistence::harness_settings::with_settings_path(settings_path, || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let mut dialog = DlgHarnessSetup {
            settings,
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
                        egui::vec2(900.0, 850.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    paint::paint_harness_setup_card(ui, dialog);
                },
            );
            output.textures_delta.clear();
            output
        };
        let output = frame(&mut dialog, vec![]);
        let retry = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Retry pi discovery" => {
                    Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
                }
                _ => None,
            })
            .expect("Pi Retry action is visible");
        crate::app::dialogs::harness_setup::discovery::test_hooks::with_worker_launch_stub(|| {
            for pressed in [true, false] {
                frame(
                    &mut dialog,
                    vec![
                        egui::Event::PointerMoved(retry),
                        egui::Event::PointerButton {
                            pos: retry,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
            assert!(dialog.scan_generation > 0);
            assert_eq!(dialog.probe_view, ProbeView::Pending);
            assert_eq!(
                crate::app::dialogs::harness_setup::discovery::test_hooks::retried_providers(),
                vec![(dialog.scan_generation, "pi".into())]
            );
        });
        let persisted = crate::persistence::harness_settings::load().0;
        assert_eq!(persisted.discovered["pi"].checked_at_unix_ms, 0);
        assert!(persisted.discovered["pi"].capability_snapshot.is_none());
        assert!(persisted.discovered["codex"].is_fresh_at(now));
        assert!(persisted.discovered["codex"].capability_snapshot.is_some());
    });
    std::fs::remove_dir_all(root).unwrap();
}
