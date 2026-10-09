use super::*;

#[test]
fn configured_model_missing_from_refreshed_catalog_fails_closed() {
    use crate::persistence::harness_settings::{DetectedHarness, HarnessSettings, WorkRoute};
    let settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            crate::persistence::harness_settings::QA.into(),
            WorkRoute {
                harness: "pi".into(),
                model: Some("removed-model".into()),
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                status: "pi ready".into(),
                version: None,
                executable: None,
                diagnostic: None,
                ready: true,
                models: vec!["available-model".into()],
                default_model: None,
                configuration_required: false,
                implementation_available: true,
            },
        )]),
        ..HarnessSettings::default()
    };
    let harness = routed_harness(
        &settings,
        Some(crate::persistence::harness_settings::QA),
        None,
        None,
    );
    let error = harness.check_available().unwrap_err().detail();
    assert!(error.contains("removed-model"));
    assert!(error.contains("refresh discovery"));
}

#[test]
fn route_label_reports_configured_default_model_before_harness_usage() {
    use crate::persistence::harness_settings::{DetectedHarness, HarnessSettings, WorkRoute};
    let settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            crate::persistence::harness_settings::QA.into(),
            WorkRoute {
                harness: "pi".into(),
                model: None,
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                status: "pi ready".into(),
                version: None,
                executable: None,
                diagnostic: None,
                ready: true,
                models: vec!["gpt-configured".into()],
                default_model: Some("gpt-configured".into()),
                configuration_required: false,
                implementation_available: true,
            },
        )]),
        ..HarnessSettings::default()
    };
    let harness = routed_harness(
        &settings,
        Some(crate::persistence::harness_settings::QA),
        None,
        None,
    );
    assert!(
        harness
            .label()
            .ends_with(" / CLI default (last discovered: gpt-configured)")
    );
}

#[test]
fn route_label_says_when_cli_does_not_report_its_default_model() {
    use crate::persistence::harness_settings::{DetectedHarness, HarnessSettings, WorkRoute};
    let settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            crate::persistence::harness_settings::QA.into(),
            WorkRoute {
                harness: "pi".into(),
                model: None,
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                status: "pi ready".into(),
                version: None,
                executable: None,
                diagnostic: None,
                ready: true,
                models: vec![],
                default_model: None,
                configuration_required: false,
                implementation_available: true,
            },
        )]),
        ..HarnessSettings::default()
    };
    let harness = routed_harness(
        &settings,
        Some(crate::persistence::harness_settings::QA),
        None,
        None,
    );
    assert!(
        harness
            .label()
            .contains("CLI default (model not reported for pi)")
    );
}
