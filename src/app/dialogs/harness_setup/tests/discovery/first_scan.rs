use super::*;

/// AC2 residue: a first-paint arm declined by the global window must keep
/// the flag pending so the next painted frame retries, and the next arm
/// once the window clears launches the sweep. Drives `ensure_first_scan`
/// directly on a pinned test clock (no sleeps); worker launch is stubbed,
/// so no host provider, Docker, or network dependency. The decline phase
/// observes a process-global stamp, so like its siblings it assumes
/// nextest-style per-test process isolation (a violation fails loudly and
/// never corrupts other tests).
#[test]
fn declined_first_paint_arm_retries_once_the_window_clears() {
    use crate::app::dialogs::harness_setup::discovery::cooldown::{
        testing_seed_last_scan_start, with_testing_now, with_window,
    };

    const WINDOW_MS: u64 = 2_000;
    const NOW: u64 = 500_000;

    with_window(WINDOW_MS, || {
        with_testing_now(NOW, || {
            // A sibling dialog instance started a scan at exactly "now".
            testing_seed_last_scan_start(NOW);

            let mut dialog = DlgHarnessSetup {
                settings: HarnessSettings::default(),
                probe_view: ProbeView::Pending,
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
                first_scan_pending: true,
            };

            dialog.ensure_first_scan();
            assert!(
                dialog.first_scan_pending,
                "a declined arm must keep the flag pending for the next frame"
            );
            let pre_launch_generation = dialog.scan_generation;
            assert_eq!(pre_launch_generation, 0);

            // Advance the clock past the window without moving the stamp:
            // the sibling's start is now older than the window and the slot
            // must free up for this dialog's retry.
            with_testing_now(NOW + WINDOW_MS + 1, || {
                crate::app::dialogs::harness_setup::discovery::test_hooks::with_worker_launch_stub(
                    || dialog.ensure_first_scan(),
                )
            });
            assert!(
                !dialog.first_scan_pending,
                "a cleared window must consume the pending flag"
            );
            assert_eq!(
                dialog.scan_generation,
                pre_launch_generation + 1,
                "the admitted arm must mint exactly one new generation"
            );
        });
    });
}

#[test]
fn stale_routed_provider_refreshes_even_when_an_unrelated_provider_is_fresh() {
    use crate::app::dialogs::harness_setup::discovery::cooldown::{with_testing_now, with_window};
    use crate::persistence::harness_settings::{
        DISCOVERY_FRESHNESS_MS, DOCUMENTATION, DetectedHarness, WorkRoute,
    };

    const NOW: u64 = 500_000;
    let mut dialog = DlgHarnessSetup {
        settings: HarnessSettings {
            default_harness: Some("codex".into()),
            work_routes: std::collections::BTreeMap::from([(
                DOCUMENTATION.into(),
                WorkRoute {
                    harness: "pi".into(),
                    model: None,
                },
            )]),
            discovered: std::collections::BTreeMap::from([
                (
                    "codex".into(),
                    DetectedHarness {
                        status: "available".into(),
                        ready: true,
                        checked_at_unix_ms: NOW,
                        ..detected("codex")
                    },
                ),
                (
                    "pi".into(),
                    DetectedHarness {
                        status: "available".into(),
                        ready: true,
                        checked_at_unix_ms: NOW.saturating_sub(DISCOVERY_FRESHNESS_MS + 1),
                        ..detected("pi")
                    },
                ),
            ]),
            ..HarnessSettings::default()
        },
        probe_view: ProbeView::Pending,
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
        first_scan_pending: true,
    };

    with_window(0, || {
        with_testing_now(NOW, || {
            crate::app::dialogs::harness_setup::discovery::test_hooks::with_worker_launch_stub(
                || dialog.ensure_first_scan(),
            )
        });
    });

    assert_ne!(
        dialog.scan_generation, 0,
        "stale documentation route must launch discovery despite fresh default evidence"
    );
    assert!(!dialog.first_scan_pending);
}

fn detected(provider: &str) -> crate::persistence::harness_settings::DetectedHarness {
    crate::persistence::harness_settings::DetectedHarness {
        status: format!("{provider} previously discovered"),
        version: None,
        executable: None,
        diagnostic: None,
        ready: false,
        models: Vec::new(),
        default_model: None,
        model_catalog_fresh: false,
        configuration_required: false,
        implementation_available: false,
        capability_snapshot: None,
        checked_at_unix_ms: 0,
    }
}
