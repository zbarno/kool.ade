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
