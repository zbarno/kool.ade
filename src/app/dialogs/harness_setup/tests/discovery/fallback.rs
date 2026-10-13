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
