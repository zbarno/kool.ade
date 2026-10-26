use super::super::{ReadinessState, discover_provider};
use std::os::unix::fs::PermissionsExt;

/// Covers a present but unusable Copilot CLI at the production adapter/settings boundary. It
/// verifies failed version discovery does not fabricate auth evidence while configured catalog
/// data stays independent. Unix shell and temporary files only; setup cost is one short CLI.
#[cfg(unix)]
#[test]
fn copilot_version_failure_keeps_auth_unknown_and_configured_models_stale() {
    let root =
        std::env::temp_dir().join(format!("koolade-copilot-adapter-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join(".copilot")).unwrap();
    std::fs::write(
        root.join(".copilot/settings.json"),
        r#"{"model":"synthetic-copilot"}"#,
    )
    .unwrap();
    let binary = root.join("copilot");
    std::fs::write(&binary, "#!/bin/sh\necho 'unrecognized CLI 1.2.3'\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::copilot_harness::COPILOT_BINARY_ENV,
        binary.as_os_str(),
    );
    environment.set("HOME", root.as_os_str());
    environment.set("KOOLADE_HOME", root.join("isolated-state").as_os_str());
    environment.remove(crate::harness::copilot_harness::COPILOT_MODEL_ENV);

    let report = discover_provider("copilot", true);
    assert!(!report.ready);
    assert_eq!(report.authentication, ReadinessState::Unknown);
    assert_eq!(report.models, ["auto", "synthetic-copilot"]);
    assert!(!report.model_catalog_fresh);
    let result = super::super::super::super::probe_result::from_harness_probe(&report, 1, 10, 1);
    assert_eq!(
        result.status,
        crate::harness::discovery::ProbeStatus::Installed
    );
    assert_eq!(
        result.failure_stage,
        Some(crate::harness::discovery::ProbeKind::Version)
    );
    assert_eq!(result.authentication, ReadinessState::Unknown);
    assert_eq!(
        result.model_catalog_status,
        crate::harness::discovery::ModelCatalogStatus::Stale
    );

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(&mut settings, &[report], None, 1);
    let detected = &settings.discovered["copilot"];
    assert!(!detected.ready);
    assert!(!detected.model_catalog_fresh);
    assert_eq!(
        detected
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        ReadinessState::Unknown
    );
    assert!(settings.default_harness.is_none());
    std::fs::remove_dir_all(root).unwrap();
}

/// Production boundary: the Copilot version probe is converted into durable settings without
/// pretending it verifies login. A parser test cannot prove readiness separation; only a
/// synthetic executable and temporary home are used, with one short child process.
#[cfg(unix)]
#[test]
fn copilot_version_success_remains_installed_until_auth_is_verified() {
    let root = std::env::temp_dir().join(format!(
        "koolade-copilot-installed-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(root.join(".copilot")).unwrap();
    std::fs::write(
        root.join(".copilot/settings.json"),
        r#"{"model":"synthetic-copilot"}"#,
    )
    .unwrap();
    let binary = root.join("copilot");
    std::fs::write(&binary, "#!/bin/sh\necho 'GitHub Copilot CLI 1.2.3'\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::copilot_harness::COPILOT_BINARY_ENV,
        binary.as_os_str(),
    );
    environment.set("HOME", root.as_os_str());
    environment.set("KOOLADE_HOME", root.join("isolated-state").as_os_str());
    environment.remove(crate::harness::copilot_harness::COPILOT_MODEL_ENV);

    let report = discover_provider("copilot", true);
    assert!(report.ready, "binary identity/version probe succeeds");
    assert_eq!(report.authentication, ReadinessState::Unknown);
    assert_eq!(report.models, ["auto", "synthetic-copilot"]);
    assert!(!report.model_catalog_fresh);
    let result = super::super::super::super::probe_result::from_harness_probe(&report, 1, 10, 1);
    assert_eq!(
        result.status,
        crate::harness::discovery::ProbeStatus::Installed
    );
    assert_eq!(
        result.execution_capability.safe_for_planning,
        ReadinessState::Unknown
    );
    assert_eq!(
        result.model_catalog_status,
        crate::harness::discovery::ModelCatalogStatus::Stale
    );

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(&mut settings, &[report], None, 1);
    let detected = &settings.discovered["copilot"];
    assert!(!detected.ready);
    assert_eq!(detected.models, ["auto", "synthetic-copilot"]);
    assert!(!detected.model_catalog_fresh);
    assert_eq!(
        detected.model_catalog_status(),
        crate::harness::discovery::ModelCatalogStatus::Stale
    );
    assert!(settings.default_harness.is_none());
    assert!(
        !super::super::super::super::has_current_provider_capabilities(
            "copilot",
            detected,
            crate::persistence::harness_settings::current_unix_ms()
        )
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// A fake executable and temporary settings file exercise catalog sanitization through durable
/// discovery state; the boundary is config-to-provider persistence and setup costs one short CLI.
#[cfg(unix)]
#[test]
fn copilot_configured_model_output_cannot_persist_secret_text() {
    let root = std::env::temp_dir().join(format!(
        "koolade-copilot-model-secret-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(root.join(".copilot")).unwrap();
    std::fs::write(
        root.join(".copilot/settings.json"),
        r#"{"model":"gpt-5 token=synthetic-secret"}"#,
    )
    .unwrap();
    let binary = root.join("copilot");
    std::fs::write(&binary, "#!/bin/sh\necho 'GitHub Copilot CLI 1.2.3'\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::copilot_harness::COPILOT_BINARY_ENV,
        binary.as_os_str(),
    );
    environment.set("HOME", root.as_os_str());
    environment.set("KOOLADE_HOME", root.join("isolated-state").as_os_str());
    environment.remove(crate::harness::copilot_harness::COPILOT_MODEL_ENV);

    let report = discover_provider("copilot", true);
    assert!(report.models.is_empty());
    assert!(!report.model_catalog_fresh);
    assert!(!report.status.contains("synthetic-secret"));
    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(&mut settings, &[report], None, 1);
    let detected = &settings.discovered["copilot"];
    assert!(detected.models.is_empty());
    assert!(!detected.model_catalog_fresh);
    assert!(!detected.status.contains("synthetic-secret"));
    std::fs::remove_dir_all(root).unwrap();
}
