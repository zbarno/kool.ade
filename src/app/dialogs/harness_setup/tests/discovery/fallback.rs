use super::*;

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
        model_catalog_fresh: false,
        configuration_required: true,
        authentication: crate::execution::contract::ReadinessState::Unavailable,
        implementation_available: false,
    };
    let mut settings = HarnessSettings::default();
    super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[pi, codex],
        Some("codex"),
        1,
    );
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
        model_catalog_fresh: true,
        configuration_required: false,
        authentication: crate::execution::contract::ReadinessState::Ready,
        implementation_available: false,
    };
    let mut settings = HarnessSettings::default();
    super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[report(true), codex],
        Some("codex"),
        1,
    );
    assert_eq!(settings.default_harness.as_deref(), Some("codex"));
}

#[test]
fn ready_executable_with_unknown_authentication_is_not_saved_as_authenticated() {
    let mut report = report(true);
    report.authentication = crate::execution::contract::ReadinessState::Unknown;
    assert!(report.ready);
    assert_eq!(
        report.authentication,
        crate::execution::contract::ReadinessState::Unknown
    );

    let mut settings = HarnessSettings::default();
    super::super::super::discovery::apply_probe_results(&mut settings, &[report], None, 1);

    assert_eq!(settings.default_harness, None);
    assert_eq!(
        settings.discovered["pi"]
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        crate::execution::contract::ReadinessState::Unknown
    );
}

#[test]
fn unrelated_provider_failure_preserves_saved_manual_path_and_default() {
    let settings_before = HarnessSettings {
        default_harness: Some("pi".into()),
        manual_executable_paths: std::collections::BTreeMap::from([
            ("pi".into(), "/example/custom/pi".into()),
            ("codex".into(), "/example/custom/codex".into()),
        ]),
        ..HarnessSettings::default()
    };
    let failure = HarnessProbe {
        id: "claude".into(),
        status: "claude (not installed)".into(),
        version: None,
        executable: None,
        diagnostic: Some("synthetic executable missing".into()),
        ready: false,
        models: Vec::new(),
        default_model: None,
        model_catalog_fresh: false,
        configuration_required: false,
        authentication: crate::execution::contract::ReadinessState::Unknown,
        implementation_available: false,
    };
    let mut settings = settings_before.clone();

    super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[failure],
        Some("claude"),
        2,
    );

    assert_eq!(settings.default_harness, settings_before.default_harness);
    assert_eq!(
        settings.manual_executable_paths,
        settings_before.manual_executable_paths
    );
    assert!(!settings.discovered["claude"].ready);
    assert!(settings.discovered["claude"].executable.is_none());
}
