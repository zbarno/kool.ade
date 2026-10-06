use super::*;
use crate::persistence::harness_settings::HarnessSettings;

fn report(ok: bool) -> crate::harness::pi_harness::ProbeReport {
    crate::harness::pi_harness::ProbeReport {
        status: if ok { "pi 0.30.1" } else { "pi (unavailable)" }.into(),
        diagnostic: if ok {
            String::new()
        } else {
            "login required".into()
        },
        binary: Some("/example/bin/pi".into()),
        ok,
        configuration_required: false,
    }
}

#[test]
fn successful_discovery_persists_version_and_selects_first_ready_default() {
    let mut settings = HarnessSettings::default();
    update_settings_from_probe(&mut settings, &report(true));
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
    update_settings_from_probe(&mut settings, &report(false));
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
    report.diagnostic = "provider credentials are missing".into();
    report.configuration_required = true;
    let mut settings = HarnessSettings::default();
    update_settings_from_probe(&mut settings, &report);
    let detected = &settings.discovered["pi"];
    assert!(!detected.ready);
    assert!(detected.configuration_required);
    assert_eq!(detected.version.as_deref(), Some("0.30.1"));
}
