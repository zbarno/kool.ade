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
                version: Some("1.0.0".into()),
                executable: Some("/synthetic/bin/pi".into()),
                diagnostic: None,
                ready: true,
                models: vec!["available-model".into()],
                default_model: None,
                model_catalog_fresh: true,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
                capability_snapshot: Some(super::authentication::ready_snapshot("pi")),
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
                version: Some("1.0.0".into()),
                executable: Some("/synthetic/bin/pi".into()),
                diagnostic: None,
                ready: true,
                models: vec!["gpt-configured".into()],
                default_model: Some("gpt-configured".into()),
                model_catalog_fresh: true,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
                capability_snapshot: Some(super::authentication::ready_snapshot("pi")),
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
    assert!(harness.label().ends_with(" / gpt-configured"));
}

#[test]
fn role_route_blocks_when_the_cli_default_model_is_unreported() {
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
                version: Some("1.0.0".into()),
                executable: Some("/synthetic/bin/pi".into()),
                diagnostic: None,
                ready: true,
                models: vec![],
                default_model: None,
                model_catalog_fresh: false,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
                capability_snapshot: Some(super::authentication::ready_snapshot("pi")),
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
    assert!(error.contains("No verified model is selected for pi"));
    assert!(error.contains("will not run with an unreported CLI default"));
}

#[test]
fn stale_discovery_cannot_route_a_saved_model() {
    use crate::persistence::harness_settings::{DetectedHarness, HarnessSettings, WorkRoute};
    let now = crate::persistence::harness_settings::current_unix_ms();
    let settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            crate::persistence::harness_settings::QA.into(),
            WorkRoute {
                harness: "pi".into(),
                model: Some("saved-model".into()),
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                status: "pi ready".into(),
                version: Some("1.0.0".into()),
                executable: Some("/tools/pi".into()),
                diagnostic: None,
                ready: true,
                models: vec!["saved-model".into()],
                default_model: Some("saved-model".into()),
                model_catalog_fresh: true,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: now.saturating_sub(
                    crate::persistence::harness_settings::DISCOVERY_FRESHNESS_MS + 1,
                ),
                capability_snapshot: Some(super::authentication::ready_snapshot("pi")),
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
    assert!(error.contains("Refresh Coding tools"));
}

#[test]
fn failed_optional_catalog_probe_does_not_treat_saved_models_as_fresh() {
    use crate::persistence::harness_settings::{DetectedHarness, HarnessSettings, WorkRoute};
    let settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            crate::persistence::harness_settings::QA.into(),
            WorkRoute {
                harness: "codex".into(),
                model: Some("saved-model".into()),
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "codex".into(),
            DetectedHarness {
                status: "codex ready".into(),
                version: Some("1.0.0".into()),
                executable: Some("/tools/codex".into()),
                diagnostic: Some("Model catalog could not be refreshed".into()),
                ready: true,
                models: vec!["saved-model".into()],
                default_model: Some("saved-model".into()),
                model_catalog_fresh: false,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
                capability_snapshot: Some(super::authentication::ready_snapshot("codex")),
            },
        )]),
        ..HarnessSettings::default()
    };
    let error = routed_harness(
        &settings,
        Some(crate::persistence::harness_settings::QA),
        None,
        None,
    )
    .check_available()
    .unwrap_err()
    .detail();
    assert!(error.contains("saved-model"));
    assert!(error.contains("refreshed"));
}
