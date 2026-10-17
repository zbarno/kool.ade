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
fn full_refresh_keeps_cached_provider_results_until_each_result_arrives() {
    let pi = report(true);
    let mut claude = report(true);
    claude.id = "claude".into();
    claude.executable = Some("/example/bin/claude".into());
    let mut settings = HarnessSettings::default();
    super::super::discovery::apply_probe_results(&mut settings, &[pi.clone(), claude], None, 1);
    let before = settings.discovered.clone();

    invalidate_discovery(&mut settings, None);

    assert_eq!(settings.discovered, before);
    assert!(settings.discovered["pi"].ready);
    assert!(settings.discovered["pi"].capability_snapshot.is_some());
    assert!(settings.discovered["claude"].ready);
    assert!(settings.discovered["claude"].capability_snapshot.is_some());
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
            probe_results: Default::default(),
            running_providers: Default::default(),
        };
        let runtime = super::super::discovery::ProbeMessage::Runtime {
            generation: 21,
            implementation_available: true,
        };
        let mut report = report(true);
        report.implementation_available = false;
        let result = super::super::probe_result::from_harness_probe(&report, 21, 100, 250);
        let provider = super::super::discovery::ProbeMessage::Provider {
            generation: 21,
            result,
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
        probe_results: Default::default(),
        running_providers: Default::default(),
    };
    let mut stale = report(false);
    stale.executable = Some("/stale/old-pi".into());
    let stale = super::super::probe_result::from_harness_probe(&stale, 7, 50, 20);
    dialog.apply_probe_message(super::super::discovery::ProbeMessage::Started {
        generation: 7,
        provider_id: "old-codex".into(),
    });
    dialog.apply_probe_message(super::super::discovery::ProbeMessage::Started {
        generation: 8,
        provider_id: "pi".into(),
    });
    dialog.apply_probe_message(super::super::discovery::ProbeMessage::Provider {
        generation: 7,
        result: stale,
    });
    assert_eq!(
        dialog.settings.discovered["pi"].executable.as_deref(),
        Some("/example/bin/pi")
    );
    assert!(dialog.settings.discovered["pi"].ready);
    assert_eq!(
        dialog.running_providers,
        std::collections::BTreeSet::from(["pi".to_owned()])
    );
    assert_eq!(dialog.probe_view, ProbeView::Pending);
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
        probe_results: Default::default(),
        running_providers: Default::default(),
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
#[path = "discovery/model_catalog.rs"]
mod model_catalog;
#[path = "discovery/progress.rs"]
mod progress;
#[path = "discovery/runtime.rs"]
mod runtime;
