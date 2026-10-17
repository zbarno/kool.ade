use super::*;

/// Production boundary: persisted readiness is invalidated before scans start, and closing the
/// dialog cancels the live generation. Reopening must not resurrect the prior authorization.
/// Uses the real settings file and dialog constructor with only worker launch stubbed; no host
/// provider, Docker, or network dependency. Setup cost is two short settings transactions.
#[test]
fn closing_and_reopening_keeps_discovery_stale_until_a_new_scan_finishes() {
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
            checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
        },
    );

    crate::persistence::harness_settings::with_settings_path(settings_path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        crate::app::dialogs::harness_setup::discovery::test_hooks::with_worker_launch_stub(|| {
            let mut dialog = DlgHarnessSetup::new();
            assert!(
                !crate::app::dialogs::harness_setup::has_current_provider_capabilities(
                    "pi",
                    &dialog.settings.discovered["pi"],
                    crate::persistence::harness_settings::current_unix_ms()
                )
            );
            let first_generation = dialog.scan_generation;
            let cancel = dialog.scan_cancel.as_ref().unwrap().clone();
            dialog.cancel_discovery();
            assert!(cancel.load(std::sync::atomic::Ordering::Acquire));
            assert!(dialog.scan_cancel.is_none());

            let reopened = DlgHarnessSetup::new();
            assert_eq!(reopened.probe_view, ProbeView::Pending);
            assert!(reopened.scan_generation > dialog.scan_generation);
            assert!(
                reopened.settings.discovered["pi"]
                    .capability_snapshot
                    .is_none()
            );
            assert_eq!(
                crate::app::dialogs::harness_setup::discovery::test_hooks::launched_generations(),
                vec![first_generation, reopened.scan_generation]
            );
        });
        let persisted = crate::persistence::harness_settings::load().0;
        assert_eq!(persisted.discovered["pi"].checked_at_unix_ms, 0);
        assert!(persisted.discovered["pi"].capability_snapshot.is_none());
    });
    std::fs::remove_dir_all(root).unwrap();
}
