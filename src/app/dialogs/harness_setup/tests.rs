use super::*;
use crate::persistence::harness_settings::HarnessSettings;

fn report(ok: bool) -> HarnessProbe {
    HarnessProbe {
        id: "pi".into(),
        status: if ok { "pi 0.30.1" } else { "pi (unavailable)" }.into(),
        diagnostic: if ok {
            None
        } else {
            Some("login required".into())
        },
        executable: Some("/example/bin/pi".into()),
        version: ok.then(|| "0.30.1".into()),
        ready: ok,
        models: vec!["configured-model".into()],
        default_model: None,
        configuration_required: false,
    }
}

#[test]
fn successful_discovery_persists_version_and_selects_first_ready_default() {
    let mut settings = HarnessSettings::default();
    apply_probe_results(&mut settings, &[report(true)], None);
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
    apply_probe_results(&mut settings, &[report(false)], None);
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
    apply_probe_results(&mut settings, &[report], None);
    let detected = &settings.discovered["pi"];
    assert!(!detected.ready);
    assert!(detected.configuration_required);
    assert_eq!(detected.version.as_deref(), Some("0.30.1"));
}

#[test]
fn discovery_keeps_adapter_failures_independent_and_uses_ready_fallback() {
    let mut pi = report(true);
    pi.id = "pi".into();
    let codex = HarnessProbe {
        id: "codex".into(),
        status: "codex (authentication required)".into(),
        version: None,
        executable: Some("/example/bin/codex".into()),
        diagnostic: Some("login required".into()),
        ready: false,
        models: vec![],
        default_model: None,
        configuration_required: true,
    };
    let mut settings = HarnessSettings::default();
    apply_probe_results(&mut settings, &[pi, codex], Some("codex"));
    assert_eq!(settings.discovered.len(), 2);
    assert!(settings.discovered["pi"].ready);
    assert!(!settings.discovered["codex"].ready);
    assert!(settings.discovered["codex"].configuration_required);
    assert_eq!(settings.default_harness.as_deref(), Some("pi"));
}

#[test]
fn configured_codex_default_is_preserved_when_ready() {
    let codex = HarnessProbe {
        id: "codex".into(),
        status: "codex 0.1.0".into(),
        version: Some("0.1.0".into()),
        executable: Some("/example/bin/codex".into()),
        diagnostic: None,
        ready: true,
        models: vec!["gpt-configured".into()],
        default_model: Some("gpt-configured".into()),
        configuration_required: false,
    };
    let mut settings = HarnessSettings::default();
    apply_probe_results(&mut settings, &[report(true), codex], Some("codex"));
    assert_eq!(settings.default_harness.as_deref(), Some("codex"));
}

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
    let previous_home = std::env::var_os("KOOLADE_HOME");
    unsafe { std::env::set_var("KOOLADE_HOME", &blocked_home) };

    let mut dialog = DlgHarnessSetup {
        settings: HarnessSettings::default(),
        probe_view: ProbeView::Complete,
        feedback: None,
        manual_path_drafts: std::collections::BTreeMap::new(),
        probe_rx: None,
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

    match previous_home {
        Some(home) => unsafe { std::env::set_var("KOOLADE_HOME", home) },
        None => unsafe { std::env::remove_var("KOOLADE_HOME") },
    }
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
    let previous_home = std::env::var_os("KOOLADE_HOME");
    unsafe { std::env::set_var("KOOLADE_HOME", &root) };
    let mut settings = HarnessSettings {
        schema_version: 2,
        manual_executable_paths: std::collections::BTreeMap::from([(
            "pi".into(),
            "/example/manual/pi".into(),
        )]),
        ..HarnessSettings::default()
    };
    apply_probe_results(&mut settings, &[report(true)], None);
    crate::persistence::harness_settings::save(&settings).unwrap();
    let mut dialog = DlgHarnessSetup {
        settings,
        probe_view: ProbeView::Complete,
        feedback: None,
        manual_path_drafts: std::collections::BTreeMap::from([(
            "pi".into(),
            "/example/manual/pi".into(),
        )]),
        probe_rx: None,
    };

    dialog.reset_manual_path("pi");

    assert!(dialog.settings.manual_executable_paths.is_empty());
    assert!(!dialog.settings.discovered.contains_key("pi"));
    assert!(matches!(dialog.probe_view, ProbeView::Pending));
    let (saved, diagnostic) = crate::persistence::harness_settings::load();
    assert!(diagnostic.is_none());
    assert!(!saved.manual_executable_paths.contains_key("pi"));
    assert!(!saved.discovered.contains_key("pi"));
    match previous_home {
        Some(home) => unsafe { std::env::set_var("KOOLADE_HOME", home) },
        None => unsafe { std::env::remove_var("KOOLADE_HOME") },
    }
    let _ = std::fs::remove_dir_all(root);
}
