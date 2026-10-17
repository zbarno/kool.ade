//! Probe provider discovery boundaries with synthetic CLI processes. These tests cover
//! adapter-to-settings mapping and provider-local failures that parser unit tests cannot
//! prove. They require Unix `/bin/sh` and temporary files only; no Docker or SSH setup is
//! needed. Each probe has a short bounded setup cost.

use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn opencode_catalog_failure_keeps_provider_ready_without_fresh_models() {
    let root =
        std::env::temp_dir().join(format!("koolade-provider-catalog-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("opencode");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'opencode 1.2.3'; exit 0; fi\necho 'provider temporarily unavailable' >&2\nexit 9\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::opencode_harness::OPENCODE_BINARY_ENV,
        binary.as_os_str(),
    );
    let report = discover_provider("opencode", true);

    assert!(report.ready);
    assert!(report.implementation_available);
    assert!(!report.configuration_required);
    assert!(!report.model_catalog_fresh);
    assert_eq!(report.version.as_deref(), Some("1.2.3"));
    assert_eq!(
        report.executable.as_deref(),
        Some(binary.to_string_lossy().as_ref())
    );
    assert!(
        report
            .diagnostic
            .as_deref()
            .unwrap()
            .to_ascii_lowercase()
            .contains("model catalog unavailable")
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn nonzero_version_probe_is_provider_local_for_all_six_supported_adapters() {
    let root = std::env::temp_dir().join(format!(
        "koolade-provider-version-failure-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("fake-cli");
    std::fs::write(
        &binary,
        "#!/bin/sh\necho synthetic-version-failure >&2\nexit 9\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    for variable in [
        crate::harness::pi_harness::PI_BINARY_ENV,
        crate::harness::codex_harness::CODEX_BINARY_ENV,
        crate::harness::claude_harness::CLAUDE_BINARY_ENV,
        crate::harness::opencode_harness::OPENCODE_BINARY_ENV,
        crate::harness::copilot_harness::COPILOT_BINARY_ENV,
        crate::harness::antigravity_harness::ANTIGRAVITY_BINARY_ENV,
    ] {
        environment.set(variable, binary.as_os_str());
    }

    for provider in PROVIDERS {
        let report = discover_provider(provider, true);
        assert_eq!(report.id, provider);
        assert!(
            !report.ready,
            "{provider} must reject a failed version probe"
        );
        assert!(
            report.executable.is_some(),
            "{provider} should retain the located executable identity"
        );
        assert!(
            report.version.is_none(),
            "{provider} must not invent a version"
        );
        let result = super::super::super::probe_result::from_harness_probe(&report, 7, 10, 1);
        assert_eq!(
            result.status,
            crate::harness::discovery::ProbeStatus::Installed,
            "{provider} failure must remain provider-local, not appear missing"
        );
        assert_eq!(
            result.failure_stage,
            Some(crate::harness::discovery::ProbeKind::Version),
            "{provider} should report the failed stage"
        );
        assert!(
            result.diagnostic_message.is_some(),
            "{provider} needs a diagnostic"
        );
    }

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn opencode_empty_success_requires_configuration_without_claiming_auth_failure() {
    let root = std::env::temp_dir().join(format!(
        "koolade-provider-empty-models-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("opencode");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'opencode 1.2.3'; exit 0; fi\nif [ \"$1\" = \"--pure\" ] && [ \"$2\" = \"models\" ]; then exit 0; fi\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::opencode_harness::OPENCODE_BINARY_ENV,
        binary.as_os_str(),
    );
    let report = discover_provider("opencode", true);

    assert!(!report.ready);
    assert!(!report.implementation_available);
    assert!(report.configuration_required);
    assert!(!report.authentication_required);
    assert_eq!(report.version.as_deref(), Some("1.2.3"));
    assert_eq!(
        report.executable.as_deref(),
        Some(binary.to_string_lossy().as_ref())
    );
    assert!(
        report
            .diagnostic
            .as_deref()
            .unwrap()
            .contains("opencode auth login")
    );

    let result = super::super::super::probe_result::from_harness_probe(&report, 1, 10, 1);
    assert_eq!(
        result.status,
        crate::harness::discovery::ProbeStatus::Installed
    );
    assert_eq!(
        result.failure_stage,
        Some(crate::harness::discovery::ProbeKind::Models)
    );
    assert_eq!(
        result.diagnostic_code,
        Some(crate::harness::discovery::ProbeDiagnosticCode::ConfigurationRequired)
    );
    assert_eq!(
        result.authentication,
        crate::execution::contract::ReadinessState::Unknown
    );
    assert_eq!(
        result.model_catalog_status,
        crate::harness::discovery::ModelCatalogStatus::Unavailable
    );

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::discovery::apply_probe_results(&mut settings, &[report], None, 1);
    assert_eq!(
        settings.discovered["opencode"]
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        crate::execution::contract::ReadinessState::Unknown
    );

    let auth_root = std::env::temp_dir().join(format!(
        "koolade-provider-auth-required-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&auth_root).unwrap();
    let auth_binary = auth_root.join("opencode");
    std::fs::write(
        &auth_binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'opencode 1.2.3'; exit 0; fi\necho 'authentication required' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&auth_binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    environment.set(
        crate::harness::opencode_harness::OPENCODE_BINARY_ENV,
        auth_binary.as_os_str(),
    );
    let auth_report = discover_provider("opencode", true);
    assert!(!auth_report.ready);
    assert!(auth_report.authentication_required);
    assert!(auth_report.configuration_required);
    super::super::super::discovery::apply_probe_results(&mut settings, &[auth_report], None, 2);
    assert_eq!(
        settings.discovered["opencode"]
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        crate::execution::contract::ReadinessState::Unavailable
    );

    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(auth_root).unwrap();
}
