use super::*;

fn available_result() -> ProbeResult {
    ProbeResult {
        provider_id: "pi".into(),
        status: ProbeStatus::Available,
        display_status: "pi 1.2".into(),
        binary_path: Some("/synthetic/bin/pi".into()),
        execution_capability: ExecutionCapability {
            safe_for_planning: ReadinessState::Ready,
            safe_for_implementation: ReadinessState::Unknown,
            runtime_ready: ReadinessState::Unknown,
        },
        ..ProbeResult::default()
    }
}

#[test]
fn provider_result_serializes_with_bounded_structured_fields() {
    let mut result = available_result();
    result.run_generation = 9;
    result.started_at_unix_ms = 1_800_000_000_000;
    result.duration_ms = 125;
    let encoded = serde_json::to_string(&result).expect("result serializes");
    let decoded: ProbeResult = serde_json::from_str(&encoded).expect("result deserializes");

    assert_eq!(decoded, result);
    assert!(encoded.contains("\"runGeneration\":9"));
    assert!(encoded.contains("\"durationMs\":125"));
}

#[test]
fn invalid_optional_catalog_does_not_hide_a_ready_provider() {
    let mut result = available_result();
    result.model_catalog_status = ModelCatalogStatus::Fresh;
    result.models = vec!["model\n-injection".into()];
    result.default_model = Some("model\n-injection".into());

    let bounded = result.bounded();

    assert_eq!(bounded.status, ProbeStatus::Available);
    assert_eq!(
        bounded.execution_capability.safe_for_planning,
        ReadinessState::Ready
    );
    assert_eq!(
        bounded.model_catalog_status,
        ModelCatalogStatus::Unavailable
    );
    assert!(bounded.models.is_empty());
    assert!(bounded.default_model.is_none());
    assert_eq!(bounded.failure_stage, Some(ProbeKind::Models));
}

#[test]
fn model_identifiers_reject_embedded_output_or_credentials() {
    for value in ["openai/gpt-5", "ollama/llama3.2", "auto", "claude-opus-4-1"] {
        assert!(super::super::is_valid_model_identifier(value), "{value}");
    }
    for value in [
        "openai/gpt token=synthetic-secret",
        "openai/gpt?token=synthetic-secret",
        "openai/gpt=synthetic-secret",
        "../gpt-5",
        "openai/../gpt-5",
        "openai//gpt-5",
        "model\nsecret",
    ] {
        assert!(!super::super::is_valid_model_identifier(value), "{value}");
    }
}

#[test]
fn invalid_binary_evidence_fails_closed_and_is_removed() {
    let mut result = available_result();
    result.binary_path = Some(format!("/{}", "x".repeat(MAX_BINARY_PATH_BYTES)));

    let bounded = result.bounded();

    assert!(bounded.binary_path.is_none());
    assert_eq!(bounded.status, ProbeStatus::Failed);
    assert_eq!(
        bounded.diagnostic_code,
        Some(ProbeDiagnosticCode::InvalidProviderResult)
    );
    assert_eq!(
        bounded.execution_capability.safe_for_planning,
        ReadinessState::Unavailable
    );
}

#[test]
fn runtime_readiness_stays_separate_from_provider_readiness() {
    let mut result = available_result();
    result.set_runtime_readiness(false);

    assert_eq!(result.status, ProbeStatus::Available);
    assert_eq!(
        result.execution_capability.safe_for_planning,
        ReadinessState::Ready
    );
    assert_eq!(
        result.execution_capability.safe_for_implementation,
        ReadinessState::Unavailable
    );
    assert_eq!(result.failure_stage, Some(ProbeKind::Capability));
    assert_eq!(
        result.diagnostic_code,
        Some(ProbeDiagnosticCode::RuntimeUnavailable)
    );
}

#[test]
fn runtime_unavailability_preserves_an_existing_provider_diagnostic() {
    let mut result = available_result();
    result.model_catalog_status = ModelCatalogStatus::Unavailable;
    result.failure_stage = Some(ProbeKind::Models);
    result.diagnostic_code = Some(ProbeDiagnosticCode::ModelCatalogUnavailable);
    result.diagnostic_message = Some("Model catalog unavailable".into());

    result.set_runtime_readiness(false);

    assert_eq!(result.failure_stage, Some(ProbeKind::Models));
    assert_eq!(
        result.diagnostic_code,
        Some(ProbeDiagnosticCode::ModelCatalogUnavailable)
    );
    assert_eq!(
        result.diagnostic_message.as_deref(),
        Some("Model catalog unavailable")
    );
    assert_eq!(
        result.execution_capability.runtime_ready,
        ReadinessState::Unavailable
    );
    assert_eq!(
        result.execution_capability.safe_for_implementation,
        ReadinessState::Unavailable
    );
}
