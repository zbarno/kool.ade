use super::*;

/// Exercises the real Settings paint path. The invariant is that an invalid
/// saved manual path is labeled and the auto-detect reset remains visible.
/// A unit test of path validation alone cannot prove the recovery control is
/// presented. This headless egui test uses only synthetic state and has no
/// external dependencies; setup cost is one frame.
#[test]
fn invalid_manual_path_shows_status_and_auto_detect_reset() {
    use crate::persistence::harness_settings::DetectedHarness;

    let mut dialog = DlgHarnessSetup {
        settings: HarnessSettings {
            default_harness: Some("pi".into()),
            manual_executable_paths: std::collections::BTreeMap::from([(
                "pi".into(),
                "/synthetic/missing/pi".into(),
            )]),
            discovered: std::collections::BTreeMap::from([(
                "pi".into(),
                DetectedHarness {
                    status: "pi (unavailable)".into(),
                    version: None,
                    executable: None,
                    diagnostic: Some("Configured path is unavailable".into()),
                    ready: false,
                    models: Vec::new(),
                    default_model: None,
                    model_catalog_fresh: false,
                    configuration_required: false,
                    implementation_available: false,
                    checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
                    capability_snapshot: None,
                },
            )]),
            ..HarnessSettings::default()
        },
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
        first_scan_pending: false,
    };
    let context = egui::Context::default();
    reveal_provider_details(&context, &mut dialog, "pi");
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        },
        |ui| {
            paint::paint_harness_setup_card(ui, &mut dialog);
        },
    );
    let labels = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(labels.iter().any(|label| label == "Invalid path"));
    assert!(labels.iter().any(|label| label == "Return to auto-detect"));
    output.textures_delta.clear();
}
