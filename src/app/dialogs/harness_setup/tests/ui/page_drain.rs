use super::*;

/// Production boundary: the Settings *page* paint pass must drain probe
/// traffic exactly like the modal card does. Before this pin, a scan armed
/// from the page leaked an unconsumed channel and the tab sat on
/// "checking…" forever (review finding folded into the thrash fix).
/// Headless egui, temporary settings file, synthetic probe channel; no host
/// provider, Docker, or network dependency; cost is two frames and two
/// queued messages.
#[test]
fn coding_settings_page_paint_consumes_probe_traffic_like_the_modal_card() {
    let state_dir =
        std::env::temp_dir().join(format!("koolade-page-drain-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&state_dir).unwrap();
    let settings_path = state_dir.join("harness-settings.json");

    crate::persistence::harness_settings::with_settings_path(settings_path, || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut dialog = DlgHarnessSetup {
            settings: HarnessSettings::default(),
            probe_view: ProbeView::Pending,
            feedback: None,
            section: HarnessSettingsSection::Tools,
            manual_path_drafts: Default::default(),
            probe_rx: Some(std::sync::Arc::new(std::sync::Mutex::new(rx))),
            scan_generation: 7,
            scan_cancel: None,
            runtime_available: None,
            scan_results: Vec::new(),
            probe_results: Default::default(),
            running_providers: Default::default(),
            first_scan_pending: false,
        };
        tx.send(super::super::super::discovery::ProbeMessage::Runtime {
            generation: 7,
            implementation_available: true,
        })
        .unwrap();

        let context = egui::Context::default();
        let paint_page = |dialog: &mut DlgHarnessSetup| {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 850.0),
                    )),
                    events: vec![],
                    ..Default::default()
                },
                |ui| {
                    crate::app::dialogs::harness_setup::paint::paint_harness_setup_page(
                        ui,
                        dialog,
                        HarnessSettingsSection::Tools,
                    );
                },
            );
            output.textures_delta.clear();
        };

        paint_page(&mut dialog);
        assert_eq!(
            dialog.runtime_available,
            Some(true),
            "the page paint pass must deliver queued runtime traffic"
        );
        assert_eq!(dialog.probe_view, ProbeView::Pending);

        tx.send(super::super::super::discovery::ProbeMessage::Finished { generation: 7 })
            .unwrap();
        paint_page(&mut dialog);
        assert_eq!(dialog.probe_view, ProbeView::Complete);
        assert!(
            dialog.probe_rx.is_none(),
            "finished generation must close the channel like the card path"
        );
    });
    std::fs::remove_dir_all(&state_dir).unwrap();
}
