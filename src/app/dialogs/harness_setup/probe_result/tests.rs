use super::*;

fn probe() -> HarnessProbe {
    HarnessProbe {
        id: "pi".into(),
        status: "pi 1.2".into(),
        version: Some("1.2".into()),
        executable: Some("/synthetic/bin/pi".into()),
        diagnostic: None,
        ready: true,
        models: vec!["model-a".into()],
        default_model: Some("model-a".into()),
        model_catalog_fresh: true,
        configuration_required: false,
        authentication: ReadinessState::Ready,
        implementation_available: false,
    }
}

#[test]
fn conversion_keeps_provider_failure_and_runtime_capability_separate() {
    let result = from_harness_probe(&probe(), 42, 1_800_000_000_000, 275);

    assert_eq!(result.provider_id, "pi");
    assert_eq!(result.run_generation, 42);
    assert_eq!(result.started_at_unix_ms, 1_800_000_000_000);
    assert_eq!(result.duration_ms, 275);
    assert_eq!(result.status, ProbeStatus::Available);
    assert_eq!(
        result.execution_capability.safe_for_planning,
        ReadinessState::Ready
    );
    assert_eq!(
        result.execution_capability.safe_for_implementation,
        ReadinessState::Unknown
    );
    assert_eq!(result.model_catalog_status, ModelCatalogStatus::Fresh);
}

#[test]
fn authentication_and_optional_catalog_failures_have_stable_codes() {
    let mut auth = probe();
    auth.ready = false;
    auth.configuration_required = true;
    auth.authentication = ReadinessState::Unavailable;
    auth.models.clear();
    auth.default_model = None;
    auth.model_catalog_fresh = false;
    auth.diagnostic = Some("Sign in at https://user:secret@example.test/login".into());
    let auth_result = from_harness_probe(&auth, 1, 10, 20);
    assert_eq!(auth_result.status, ProbeStatus::NeedsAuthentication);
    assert_eq!(
        auth_result.diagnostic_code,
        Some(ProbeDiagnosticCode::AuthenticationRequired)
    );
    assert!(auth_result.models.is_empty());
    assert_eq!(
        auth_result.model_catalog_status,
        ModelCatalogStatus::NotChecked
    );
    assert!(
        auth_result
            .diagnostic_message
            .as_deref()
            .is_some_and(|text| text.contains("[REDACTED]@example.test"))
    );
    assert!(
        !auth_result
            .diagnostic_message
            .as_deref()
            .is_some_and(|text| text.contains("secret"))
    );

    let mut catalog = probe();
    catalog.diagnostic = Some("Model catalog unavailable".into());
    catalog.model_catalog_fresh = false;
    catalog.models.clear();
    let catalog_result = from_harness_probe(&catalog, 2, 20, 30);
    assert_eq!(catalog_result.status, ProbeStatus::Installed);
    assert_eq!(
        catalog_result.execution_capability.safe_for_planning,
        ReadinessState::Unknown
    );
    assert_eq!(
        catalog_result.model_catalog_status,
        ModelCatalogStatus::Unavailable
    );
    assert_eq!(catalog_result.failure_stage, Some(ProbeKind::Models));
}

#[test]
fn a_fresh_empty_catalog_does_not_make_an_authenticated_provider_available() {
    let mut empty = probe();
    empty.models.clear();
    empty.default_model = None;

    let result = from_harness_probe(&empty, 15, 100, 5);

    assert_eq!(result.status, ProbeStatus::Installed);
    assert_eq!(
        result.execution_capability.safe_for_planning,
        ReadinessState::Unknown
    );
    assert_eq!(result.model_catalog_status, ModelCatalogStatus::Fresh);
}

#[test]
fn claude_can_be_authenticated_but_task_execution_remains_blocked() {
    let mut source = probe();
    source.id = "claude".into();

    let result = from_harness_probe(&source, 16, 100, 5);

    assert_eq!(result.status, ProbeStatus::Installed);
    assert_eq!(result.authentication, ReadinessState::Ready);
    assert_eq!(
        result.execution_capability.safe_for_planning,
        ReadinessState::Unavailable
    );
    assert_eq!(
        result.execution_capability.safe_for_implementation,
        ReadinessState::Unavailable
    );
    assert!(
        result
            .diagnostic_message
            .as_deref()
            .is_some_and(|message| message.contains("organization-managed hooks"))
    );
}

#[test]
fn retained_models_are_reported_as_stale_and_never_fresh() {
    let mut catalog = probe();
    catalog.diagnostic = Some("Model catalog unavailable".into());
    catalog.model_catalog_fresh = false;
    let result = from_harness_probe(&catalog, 3, 30, 40);

    assert_eq!(result.model_catalog_status, ModelCatalogStatus::Stale);
    assert_eq!(result.models, ["model-a"]);
    assert!(!into_harness_probe(&result).model_catalog_fresh);
}

#[test]
fn typed_result_converts_back_to_legacy_saved_settings() {
    let mut source = probe();
    source.implementation_available = true;
    let result = from_harness_probe(&source, 8, 100, 50);
    let legacy = into_harness_probe(&result);

    assert_eq!(legacy.id, source.id);
    assert_eq!(legacy.executable, source.executable);
    assert_eq!(legacy.models, source.models);
    assert!(legacy.ready);
    assert!(legacy.implementation_available);
    assert!(legacy.model_catalog_fresh);
}

#[test]
fn unknown_authentication_never_becomes_ready_implementation_capability() {
    let mut source = probe();
    source.authentication = ReadinessState::Unknown;
    source.implementation_available = true;

    let result = from_harness_probe(&source, 9, 100, 50);
    let legacy = into_harness_probe(&result);

    assert_eq!(result.status, ProbeStatus::Installed);
    assert_eq!(result.authentication, ReadinessState::Unknown);
    assert_eq!(
        result.execution_capability.safe_for_planning,
        ReadinessState::Unknown
    );
    assert_eq!(
        result.execution_capability.safe_for_implementation,
        ReadinessState::Unknown
    );
    assert_eq!(legacy.authentication, ReadinessState::Unknown);
    assert!(!legacy.ready);
    assert!(!legacy.implementation_available);

    let mut settings = crate::persistence::harness_settings::HarnessSettings {
        schema_version: 3,
        ..Default::default()
    };
    super::super::discovery::apply_probe_results(&mut settings, &[source], None, 9);
    let path = std::env::temp_dir().join(format!("harness-auth-{}.json", uuid::Uuid::new_v4()));
    crate::persistence::harness_settings::with_settings_path(path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let (loaded, diagnostic) = crate::persistence::harness_settings::load();
        assert!(diagnostic.is_none());
        let detected = &loaded.discovered["pi"];
        assert!(!detected.ready);
        assert_eq!(
            detected
                .capability_snapshot
                .as_ref()
                .unwrap()
                .authentication,
            ReadinessState::Unknown
        );
        assert!(!super::super::has_current_provider_capabilities(
            "pi",
            detected,
            crate::persistence::harness_settings::current_unix_ms()
        ));
    });
    let _ = std::fs::remove_file(path);
}

#[test]
fn configuration_requirement_roundtrip_preserves_unknown_authentication() {
    let mut source = probe();
    source.authentication = ReadinessState::Unknown;
    source.configuration_required = true;
    source.ready = false;

    let result = from_harness_probe(&source, 10, 100, 50);
    let legacy = into_harness_probe(&result);

    assert_eq!(result.authentication, ReadinessState::Unknown);
    assert_eq!(
        result.diagnostic_code,
        Some(ProbeDiagnosticCode::ConfigurationRequired)
    );
    assert_eq!(legacy.authentication, ReadinessState::Unknown);
    assert!(legacy.configuration_required);
}

#[test]
fn internal_probe_failure_is_typed_without_claiming_the_provider_is_missing() {
    let result = internal_failure_result("pi", 12, 100, 8);

    assert_eq!(result.provider_id, "pi");
    assert_eq!(result.run_generation, 12);
    assert_eq!(result.status, ProbeStatus::Failed);
    assert_eq!(result.failure_stage, Some(ProbeKind::Capability));
    assert_eq!(
        result.diagnostic_code,
        Some(ProbeDiagnosticCode::InternalProbeFailure)
    );
    assert_eq!(result.binary_path, None);
    assert_eq!(
        result.execution_capability.safe_for_planning,
        ReadinessState::Unavailable
    );
}

#[test]
fn invalid_user_path_is_not_mislabeled_as_absent_software() {
    let mut source = probe();
    source.status = "pi (invalid executable path)".into();
    source.executable = None;
    source.ready = false;
    source.authentication = ReadinessState::Unknown;
    source.diagnostic = Some("The configured executable path is invalid.".into());

    let result = from_harness_probe(&source, 13, 100, 5);

    assert_eq!(result.status, ProbeStatus::Failed);
    assert_eq!(result.failure_stage, Some(ProbeKind::Executable));
    assert_eq!(
        result.diagnostic_code,
        Some(ProbeDiagnosticCode::ExecutableInvalid)
    );
    assert_eq!(
        result.execution_capability.safe_for_planning,
        ReadinessState::Unavailable
    );
}

#[test]
fn provider_profile_mismatch_is_an_unsupported_version_not_ready() {
    let mut source = probe();
    source.status = "codex (installed but unusable)".into();
    source.ready = false;
    source.diagnostic =
        Some("Codex CLI 0.1 is unsupported for clean execution; missing options: --sandbox".into());

    let result = from_harness_probe(&source, 14, 100, 5);

    assert_eq!(result.status, ProbeStatus::Installed);
    assert_eq!(result.failure_stage, Some(ProbeKind::Version));
    assert_eq!(
        result.diagnostic_code,
        Some(ProbeDiagnosticCode::UnsupportedVersion)
    );
    assert_eq!(
        result.execution_capability.safe_for_planning,
        ReadinessState::Unavailable
    );
}
