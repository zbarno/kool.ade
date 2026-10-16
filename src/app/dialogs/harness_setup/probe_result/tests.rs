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
        authentication_required: false,
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
    auth.authentication_required = true;
    auth.diagnostic = Some("Sign in at https://user:secret@example.test/login".into());
    let auth_result = from_harness_probe(&auth, 1, 10, 20);
    assert_eq!(auth_result.status, ProbeStatus::NeedsAuthentication);
    assert_eq!(
        auth_result.diagnostic_code,
        Some(ProbeDiagnosticCode::AuthenticationRequired)
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
    assert_eq!(catalog_result.status, ProbeStatus::Available);
    assert_eq!(
        catalog_result.execution_capability.safe_for_planning,
        ReadinessState::Ready
    );
    assert_eq!(
        catalog_result.model_catalog_status,
        ModelCatalogStatus::Unavailable
    );
    assert_eq!(catalog_result.failure_stage, Some(ProbeKind::Models));
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
