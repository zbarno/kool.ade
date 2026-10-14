use super::*;

#[test]
fn invalidating_a_manual_provider_keeps_only_stale_diagnostics() {
    let mut settings = HarnessSettings::default();
    super::super::discovery::apply_probe_results(&mut settings, &[report(true)], None, 1);
    let snapshot = settings.discovered["pi"]
        .capability_snapshot
        .as_ref()
        .unwrap();
    assert_eq!(
        snapshot.modes.len(),
        crate::harness::ExecutionMode::ALL.len()
    );
    assert_eq!(
        snapshot.roles.len(),
        crate::execution::contract::HarnessRole::ALL.len()
    );
    assert_eq!(
        snapshot.docker_mcp_bridge,
        crate::execution::contract::ReadinessState::Unavailable
    );
    assert_eq!(
        snapshot.execution_target,
        crate::execution::contract::ReadinessState::Unknown
    );
    assert!(!snapshot.supports_role(crate::execution::contract::HarnessRole::Implementer));
    assert!(
        settings.discovered["pi"]
            .is_fresh_at(crate::persistence::harness_settings::current_unix_ms())
    );

    invalidate_discovery(&mut settings, Some("pi"));

    let detected = &settings.discovered["pi"];
    assert!(!detected.is_fresh_at(crate::persistence::harness_settings::current_unix_ms()));
    assert!(!detected.implementation_available);
    assert!(detected.capability_snapshot.is_none());
    assert_eq!(detected.version.as_deref(), Some("0.30.1"));
}

#[test]
fn runtime_probe_updates_only_the_current_provider_capability_snapshot() {
    use crate::execution::contract::ReadinessState;

    let mut settings = HarnessSettings::default();
    super::super::discovery::apply_probe_results(&mut settings, &[report(true)], None, 11);
    let mut reports = vec![report(true)];
    super::super::discovery::update_runtime_readiness(&mut settings, &mut reports, 11, true);

    super::super::discovery::update_runtime_readiness(&mut settings, &mut reports, 10, true);
    assert_eq!(
        settings.discovered["pi"]
            .capability_snapshot
            .as_ref()
            .unwrap()
            .execution_target,
        ReadinessState::Ready,
        "a prior scan cannot overwrite current runtime readiness"
    );

    super::super::discovery::update_runtime_readiness(&mut settings, &mut reports, 11, true);
    assert_eq!(
        settings.discovered["pi"]
            .capability_snapshot
            .as_ref()
            .unwrap()
            .execution_target,
        ReadinessState::Ready
    );
}

#[test]
fn setup_selection_requires_a_valid_current_capability_snapshot() {
    let generation = crate::persistence::harness_settings::current_unix_ms();
    let mut settings = HarnessSettings::default();
    super::super::discovery::apply_probe_results(&mut settings, &[report(true)], None, generation);
    // Probe results stamp their own clock value. Sample afterward so a
    // millisecond rollover cannot make the supposedly current snapshot look
    // like a future result during this assertion.
    let now = crate::persistence::harness_settings::current_unix_ms();
    let entry = &settings.discovered["pi"];
    assert!(super::super::has_current_provider_capabilities(
        "pi", entry, now
    ));

    let mut invalid = entry.clone();
    invalid.capability_snapshot.as_mut().unwrap().provider_id = "codex".into();
    assert!(!super::super::has_current_provider_capabilities(
        "pi", &invalid, now
    ));

    let mut stale = entry.clone();
    stale.checked_at_unix_ms = 0;
    assert!(!super::super::has_current_provider_capabilities(
        "pi", &stale, now
    ));
}

#[test]
fn runtime_and_provider_messages_can_arrive_in_either_order() {
    use crate::execution::contract::ReadinessState;

    for runtime_first in [false, true] {
        let mut dialog = DlgHarnessSetup {
            settings: HarnessSettings::default(),
            probe_view: ProbeView::Pending,
            feedback: None,
            section: HarnessSettingsSection::Tools,
            manual_path_drafts: Default::default(),
            probe_rx: None,
            scan_generation: 21,
            scan_cancel: None,
            runtime_available: None,
            scan_results: Vec::new(),
        };
        let runtime = super::super::discovery::ProbeMessage::Runtime {
            generation: 21,
            implementation_available: true,
        };
        let mut report = report(true);
        report.implementation_available = false;
        let provider = super::super::discovery::ProbeMessage::Provider {
            generation: 21,
            report,
        };
        if runtime_first {
            dialog.apply_probe_message(runtime);
            dialog.apply_probe_message(provider);
        } else {
            dialog.apply_probe_message(provider);
            dialog.apply_probe_message(runtime);
        }
        let detected = &dialog.settings.discovered["pi"];
        assert!(detected.implementation_available);
        assert_eq!(
            detected
                .capability_snapshot
                .as_ref()
                .unwrap()
                .execution_target,
            ReadinessState::Ready
        );
    }
}

#[test]
fn late_probe_from_previous_generation_cannot_replace_saved_result() {
    let mut settings = HarnessSettings::default();
    super::super::discovery::apply_probe_results(&mut settings, &[report(true)], None, 1);
    let mut dialog = DlgHarnessSetup {
        settings,
        probe_view: ProbeView::Pending,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: Default::default(),
        probe_rx: None,
        scan_generation: 8,
        scan_cancel: None,
        runtime_available: None,
        scan_results: Vec::new(),
    };
    let mut stale = report(false);
    stale.executable = Some("/stale/old-pi".into());
    dialog.apply_probe_message(super::super::discovery::ProbeMessage::Provider {
        generation: 7,
        report: stale,
    });
    assert_eq!(
        dialog.settings.discovered["pi"].executable.as_deref(),
        Some("/example/bin/pi")
    );
    assert!(dialog.settings.discovered["pi"].ready);
    assert_eq!(dialog.probe_view, ProbeView::Pending);
}

#[test]
fn pending_scan_keeps_completed_provider_results_visible() {
    let mut settings = HarnessSettings::default();
    super::super::discovery::apply_probe_results(&mut settings, &[report(true)], None, 1);
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
        scan_results: vec![report(true)],
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
    assert!(
        labels
            .iter()
            .any(|label| label.contains("Still checking: codex"))
    );
    assert!(labels.iter().any(|label| label.starts_with("pi")));
}

#[test]
fn closing_discovery_cancels_the_active_provider_scan() {
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut dialog = DlgHarnessSetup {
        settings: HarnessSettings::default(),
        probe_view: ProbeView::Pending,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: Default::default(),
        probe_rx: None,
        scan_generation: 1,
        scan_cancel: Some(cancel.clone()),
        runtime_available: None,
        scan_results: Vec::new(),
    };
    dialog.cancel_discovery();
    assert!(cancel.load(std::sync::atomic::Ordering::Acquire));
    assert!(dialog.scan_cancel.is_none());
    assert_eq!(dialog.probe_view, ProbeView::Complete);
}

#[test]
fn successful_discovery_persists_version_and_selects_first_ready_default() {
    let mut settings = HarnessSettings::default();
    super::super::discovery::apply_probe_results(&mut settings, &[report(true)], None, 1);
    assert_eq!(settings.default_harness.as_deref(), Some("pi"));
    assert_eq!(settings.discovered["pi"].version.as_deref(), Some("0.30.1"));
    assert!(settings.discovered["pi"].ready);
}

#[test]
fn rediscovery_reflects_tool_removal_without_rewriting_the_saved_default() {
    let mut settings = HarnessSettings {
        default_harness: Some("pi".into()),
        ..HarnessSettings::default()
    };
    super::super::discovery::apply_probe_results(&mut settings, &[report(false)], None, 1);
    assert_eq!(settings.default_harness.as_deref(), Some("pi"));
    assert!(!settings.discovered["pi"].ready);
    assert_eq!(
        settings.discovered["pi"].diagnostic.as_deref(),
        Some("login required")
    );
}

#[test]
fn missing_provider_setup_is_recorded_separately_from_a_missing_executable() {
    let mut report = report(false);
    report.status = "pi 0.30.1".into();
    report.diagnostic = Some("provider credentials are missing".into());
    report.version = Some("0.30.1".into());
    report.ready = false;
    report.configuration_required = true;
    let mut settings = HarnessSettings::default();
    super::super::discovery::apply_probe_results(&mut settings, &[report], None, 1);
    let detected = &settings.discovered["pi"];
    assert!(!detected.ready);
    assert!(detected.configuration_required);
    assert_eq!(detected.version.as_deref(), Some("0.30.1"));
}

#[path = "discovery/fallback.rs"]
mod fallback;
