use super::*;

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
    assert_eq!(
        report.authentication,
        crate::execution::contract::ReadinessState::Unknown
    );
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

    let result = super::super::super::super::probe_result::from_harness_probe(&report, 1, 10, 1);
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
    super::super::super::super::discovery::apply_probe_results(&mut settings, &[report], None, 1);
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
    // The executable and catalog command are present, but a failed auth-list command cannot
    // prove whether credentials exist. Preserve installed status while keeping routing denied.
    assert!(auth_report.ready);
    assert_eq!(
        auth_report.authentication,
        crate::execution::contract::ReadinessState::Unknown
    );
    assert!(!auth_report.configuration_required);
    super::super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[auth_report],
        None,
        2,
    );
    assert_eq!(
        settings.discovered["opencode"]
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        crate::execution::contract::ReadinessState::Unknown
    );
    assert!(settings.default_harness.is_none());
    assert!(
        !super::super::super::super::has_current_provider_capabilities(
            "opencode",
            &settings.discovered["opencode"],
            crate::persistence::harness_settings::current_unix_ms()
        )
    );

    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(auth_root).unwrap();
}
