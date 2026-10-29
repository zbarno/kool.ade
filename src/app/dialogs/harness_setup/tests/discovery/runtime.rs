use super::*;

#[test]
fn runtime_probe_refreshes_all_retained_snapshots_but_stale_messages_are_ignored() {
    use crate::execution::contract::ReadinessState;

    let mut settings = HarnessSettings::default();
    let pi = report(true);
    let mut codex = report(true);
    codex.id = "codex".into();
    codex.status = "codex 1.0".into();
    codex.executable = Some("/example/bin/codex".into());
    codex.version = Some("1.0".into());
    super::super::super::discovery::apply_probe_results(
        &mut settings,
        std::slice::from_ref(&pi),
        None,
        10,
    );
    super::super::super::discovery::apply_probe_results(
        &mut settings,
        std::slice::from_ref(&codex),
        None,
        11,
    );
    let mut dialog = DlgHarnessSetup {
        settings,
        probe_view: ProbeView::Pending,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: Default::default(),
        probe_rx: None,
        scan_generation: 12,
        scan_cancel: None,
        runtime_available: None,
        scan_results: vec![pi.clone(), codex.clone()],
        probe_results: std::collections::BTreeMap::from([
            (
                "pi".into(),
                super::super::super::probe_result::from_harness_probe(&pi, 10, 100, 20),
            ),
            (
                "codex".into(),
                super::super::super::probe_result::from_harness_probe(&codex, 11, 110, 25),
            ),
        ]),
        running_providers: Default::default(),
        first_scan_pending: false,
    };

    dialog.apply_probe_message(super::super::super::discovery::ProbeMessage::Runtime {
        generation: 12,
        implementation_available: true,
    });
    for id in ["pi", "codex"] {
        let entry = &dialog.settings.discovered[id];
        let snapshot = entry.capability_snapshot.as_ref().unwrap();
        assert_eq!(snapshot.execution_target, ReadinessState::Ready);
        assert_eq!(
            snapshot.probe_cancellation_generation,
            if id == "pi" { 10 } else { 11 },
            "refreshing global runtime readiness must not relabel provider discovery"
        );
        assert_eq!(
            dialog.probe_results[id].execution_capability.runtime_ready,
            ReadinessState::Ready
        );
    }

    dialog.apply_probe_message(super::super::super::discovery::ProbeMessage::Runtime {
        generation: 11,
        implementation_available: false,
    });
    assert_eq!(
        dialog.settings.discovered["pi"]
            .capability_snapshot
            .as_ref()
            .unwrap()
            .execution_target,
        ReadinessState::Ready,
        "a previous scan cannot overwrite the current global runtime result"
    );
}
