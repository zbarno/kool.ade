use super::*;

/// Production boundary: construction never starts a sweep (the thrash that
/// pinned the dialog on "checking…"), the first painted frame arms one only
/// when persisted evidence is stale, close cancels the live generation, and
/// reopening serves the cached capability without dropping it. Uses the real
/// settings file and dialog constructor with only worker launch stubbed; no
/// host provider, Docker, or network dependency. Setup cost is two short
/// settings transactions.
#[test]
fn closing_and_reopening_keeps_cached_discovery_until_a_new_result_arrives() {
    let root = std::env::temp_dir().join(format!(
        "koolade-discovery-close-reopen-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let settings_path = root.join("harness-settings.json");
    let mut snapshot = super::codex_catalog_snapshot();
    snapshot.provider_id = "pi".into();
    let mut settings = HarnessSettings {
        default_harness: Some("pi".into()),
        ..HarnessSettings::default()
    };
    settings.discovered.insert(
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
            capability_snapshot: Some(snapshot),
            checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms()
                .saturating_sub(crate::persistence::harness_settings::DISCOVERY_FRESHNESS_MS + 1),
        },
    );

    crate::persistence::harness_settings::with_settings_path(settings_path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        crate::app::dialogs::harness_setup::discovery::test_hooks::with_worker_launch_stub(|| {
            // Zero-width window: both first-paint arms below must be admitted
            // back-to-back; production-window admission is pinned by the
            // cooldown unit tests and refresh_click's decline test.
            crate::app::dialogs::harness_setup::discovery::cooldown::with_window(0, || {
                let mut dialog = DlgHarnessSetup::new();
                assert!(
                    dialog.settings.discovered["pi"]
                        .capability_snapshot
                        .is_some(),
                    "cached capability must survive the reload"
                );
                // Regression guard: construction must not mint a generation.
                assert_eq!(dialog.scan_generation, 0);
                assert!(dialog.scan_cancel.is_none());

                dialog.ensure_first_scan();
                let first_generation = dialog.scan_generation;
                assert_ne!(
                    first_generation, 0,
                    "stale evidence must arm a first-paint scan"
                );
                let cancel = dialog.scan_cancel.as_ref().unwrap().clone();
                dialog.cancel_discovery();
                assert!(cancel.load(std::sync::atomic::Ordering::Acquire));
                assert!(dialog.scan_cancel.is_none());

                let mut reopened = DlgHarnessSetup::new();
                assert_eq!(reopened.probe_view, ProbeView::Pending);
                assert_eq!(
                    reopened.scan_generation, 0,
                    "reopen must not scan at construction"
                );
                reopened.ensure_first_scan();
                assert!(
                    reopened.settings.discovered["pi"]
                        .capability_snapshot
                        .is_some()
                );
                assert_eq!(
                    crate::app::dialogs::harness_setup::discovery::test_hooks::launched_generations(
                    ),
                    vec![first_generation, reopened.scan_generation]
                );
            });
        });
        let persisted = crate::persistence::harness_settings::load().0;
        assert!(persisted.discovered["pi"].checked_at_unix_ms > 0);
        assert!(persisted.discovered["pi"].capability_snapshot.is_some());
    });
    std::fs::remove_dir_all(root).unwrap();
}
