use super::super::super::{
    DlgHarnessSetup, HarnessSettingsSection, ProbeView, discovery, paint, probe_result,
};
use super::super::{HarnessSettings, report};

#[test]
fn pending_scan_keeps_completed_provider_results_visible() {
    let mut settings = HarnessSettings::default();
    let report = report(true);
    discovery::apply_probe_results(&mut settings, std::slice::from_ref(&report), None, 1);
    let mut dialog = DlgHarnessSetup {
        settings,
        probe_view: ProbeView::Pending,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: Default::default(),
        probe_rx: None,
        scan_generation: 1,
        scan_cancel: None,
        runtime_available: None,
        scan_results: vec![report.clone()],
        probe_results: std::collections::BTreeMap::from([(
            "pi".into(),
            probe_result::from_harness_probe(&report, 1, 1_800, 57),
        )]),
        running_providers: std::collections::BTreeSet::from(["codex".into()]),
        first_scan_pending: false,
    };
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(
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
    output.textures_delta.clear();
    assert!(labels.iter().any(|label| {
        label == "Checking installed tools; completed provider results appear below."
    }));
    assert!(labels.iter().any(|label| label == "Checking 1 tools…"));
    assert!(labels.iter().any(|label| label == "Add / Fix a coding tool"));
    // Completed but unverified provider details are still accessible under
    // Add / Fix, not advertised as ready in the main picker.
    assert!(labels.iter().any(|label| label.starts_with("pi")));
}
