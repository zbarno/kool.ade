use super::*;

#[cfg(unix)]
#[test]
fn manual_path_save_and_reset_roll_back_memory_when_settings_cannot_be_written() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!(
        "koolade-harness-settings-failure-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let blocked_home = root.join("home-is-a-file");
    std::fs::write(&blocked_home, "not a directory").unwrap();
    let binary = root.join("pi");
    std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _home = crate::test_support::KooladeHomeGuard::set(&blocked_home);
    let mut dialog = DlgHarnessSetup {
        settings: HarnessSettings::default(),
        probe_view: ProbeView::Complete,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: std::collections::BTreeMap::new(),
        probe_rx: None,
        scan_generation: 0,
        scan_cancel: None,
        runtime_available: None,
        scan_results: Vec::new(),
    };
    dialog.set_manual_path("pi", binary.display().to_string());
    assert!(!dialog.settings.manual_executable_paths.contains_key("pi"));
    assert!(dialog.feedback.as_ref().is_some_and(|(ok, _)| !ok));

    dialog
        .settings
        .manual_executable_paths
        .insert("pi".into(), binary.display().to_string());
    dialog
        .manual_path_drafts
        .insert("pi".into(), binary.display().to_string());
    let saved = dialog.settings.clone();
    dialog.reset_manual_path("pi");
    assert_eq!(dialog.settings, saved);
    assert_eq!(
        dialog.manual_path_drafts["pi"],
        binary.display().to_string()
    );
    assert!(dialog.feedback.as_ref().is_some_and(|(ok, _)| !ok));

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn reset_clears_manual_discovery_until_automatic_probe_finishes() {
    let root = std::env::temp_dir().join(format!(
        "koolade-harness-reset-discovery-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let mut settings = HarnessSettings {
        schema_version: 2,
        manual_executable_paths: std::collections::BTreeMap::from([(
            "pi".into(),
            "/example/manual/pi".into(),
        )]),
        ..HarnessSettings::default()
    };
    super::super::discovery::apply_probe_results(&mut settings, &[report(true)], None, 1);
    crate::persistence::harness_settings::save(&settings).unwrap();
    let (persisted, diagnostic) = crate::persistence::harness_settings::load();
    assert!(diagnostic.is_none());
    let snapshot = persisted.discovered["pi"]
        .capability_snapshot
        .as_ref()
        .unwrap();
    assert_eq!(
        snapshot.probe_cancellation_generation, 1,
        "the capability snapshot retains the accepted probe generation"
    );
    assert_eq!(
        snapshot.roles.len(),
        crate::execution::contract::HarnessRole::ALL.len()
    );
    let mut dialog = DlgHarnessSetup {
        settings,
        probe_view: ProbeView::Complete,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: std::collections::BTreeMap::from([(
            "pi".into(),
            "/example/manual/pi".into(),
        )]),
        probe_rx: None,
        scan_generation: 0,
        scan_cancel: None,
        runtime_available: None,
        scan_results: Vec::new(),
    };

    dialog.reset_manual_path("pi");

    assert!(dialog.settings.manual_executable_paths.is_empty());
    assert!(!dialog.settings.discovered.contains_key("pi"));
    assert!(matches!(dialog.probe_view, ProbeView::Pending));
    let (saved, diagnostic) = crate::persistence::harness_settings::load();
    assert!(diagnostic.is_none());
    assert!(!saved.manual_executable_paths.contains_key("pi"));
    assert!(!saved.discovered.contains_key("pi"));
    let _ = std::fs::remove_dir_all(root);
}
