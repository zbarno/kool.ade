use crate::execution::contract::{
    ArtifactManifestEntryV1, ExecutionResultV1, HarnessRole, ModeReadinessV1, ProviderReadinessV1,
    ReadinessState, ResourceMetricsV1, RoleReadinessV1,
};
use crate::execution::errors::{ExecutionError, ExecutionErrorKind};
use crate::harness::ExecutionMode;

fn id() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

#[test]
fn provider_readiness_has_cancellable_versioned_probes_and_unique_roles() {
    let mut readiness = ProviderReadinessV1 {
        schema_version: 1,
        provider_id: "provider-a".into(),
        probe_id: id(),
        probe_cancellation_generation: 1,
        probe_sequence: 1,
        cli_binary: ReadinessState::Ready,
        cli_version: Some("1.2.3".into()),
        authentication: ReadinessState::Ready,
        model_catalog: ReadinessState::Ready,
        scriptable_invocation: ReadinessState::Ready,
        docker_mcp_bridge: ReadinessState::Unknown,
        execution_target: ReadinessState::Unavailable,
        failure: None,
        modes: ExecutionMode::ALL
            .into_iter()
            .map(|mode| ModeReadinessV1 {
                mode,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
        roles: HarnessRole::ALL
            .into_iter()
            .map(|role| RoleReadinessV1 {
                role,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
    };
    readiness.validate().unwrap();
    readiness.roles.pop();
    assert!(readiness.validate().is_err());
    readiness.roles.push(RoleReadinessV1 {
        role: HarnessRole::PRPreparer,
        available: ReadinessState::Unavailable,
        failure: Some(ExecutionErrorKind::EnvironmentUnavailable),
    });
    readiness.validate().unwrap();
    readiness.roles.push(readiness.roles[0].clone());
    assert!(readiness.validate().is_err());
    assert_eq!(
        HarnessRole::PRPreparer.harness_mode(),
        crate::harness::ExecutionMode::ReadOnlyAnalysis
    );
    assert_eq!(
        HarnessRole::PRPreparer.execution_mode(),
        crate::execution::contract::ExecutionModeV1::PlanningReadOnly
    );
}

#[test]
fn canonical_harness_roles_preserve_legacy_serialized_names() {
    use HarnessRole::*;
    let legacy = [
        ("specification", PlanningAnalyst),
        ("investigation", PlanningAnalyst),
        ("decision_explanation", PlanReviewer),
        ("task_generation", TaskDecomposer),
        ("implementation", Implementer),
        ("quality_assurance", RuntimeQaReviewer),
        ("documentation", PRPreparer),
        ("reconciliation", SpecReconciler),
    ];
    for (serialized, expected) in legacy {
        let actual: HarnessRole = serde_json::from_str(&format!("\"{serialized}\"")).unwrap();
        assert_eq!(actual, expected, "legacy role {serialized}");
    }
    assert_eq!(HarnessRole::ALL.len(), 11);
    assert_eq!(HarnessRole::ALL[0], Manager);
    assert_eq!(HarnessRole::ALL[10], PRPreparer);
    assert_eq!(
        serde_json::to_string(&DecisionExplainer).unwrap(),
        "\"decision_explainer\""
    );
}

#[test]
fn provider_role_routing_requires_only_the_authority_needed_by_that_role() {
    let mut readiness = ProviderReadinessV1 {
        schema_version: 1,
        provider_id: "provider-a".into(),
        probe_id: id(),
        probe_cancellation_generation: 1,
        probe_sequence: 1,
        cli_binary: ReadinessState::Ready,
        cli_version: Some("1.2.3".into()),
        authentication: ReadinessState::Ready,
        model_catalog: ReadinessState::Ready,
        scriptable_invocation: ReadinessState::Ready,
        docker_mcp_bridge: ReadinessState::Unavailable,
        execution_target: ReadinessState::Unavailable,
        failure: None,
        modes: ExecutionMode::ALL
            .into_iter()
            .map(|mode| ModeReadinessV1 {
                mode,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
        roles: HarnessRole::ALL
            .into_iter()
            .map(|role| RoleReadinessV1 {
                role,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
    };

    assert!(readiness.supports_role(HarnessRole::SpecReconciler));
    assert!(!readiness.supports_role(HarnessRole::PlanReviewer));
    assert!(!readiness.supports_role(HarnessRole::RuntimeQaReviewer));
    assert!(readiness.supports_mode(ExecutionMode::ReadOnlyAnalysis));
    assert!(!readiness.supports_mode(ExecutionMode::Planning));
    assert!(!readiness.supports_role(HarnessRole::Implementer));
    readiness.docker_mcp_bridge = ReadinessState::Ready;
    readiness.execution_target = ReadinessState::Ready;
    assert!(readiness.supports_role(HarnessRole::PlanReviewer));
    assert!(readiness.supports_role(HarnessRole::RuntimeQaReviewer));
    assert!(readiness.supports_mode(ExecutionMode::Planning));
    assert!(readiness.supports_role(HarnessRole::Implementer));
    readiness.authentication = ReadinessState::Unknown;
    assert!(!readiness.supports_role(HarnessRole::Implementer));
    readiness.authentication = ReadinessState::Ready;
    readiness
        .modes
        .iter_mut()
        .find(|mode| mode.mode == ExecutionMode::Implementation)
        .unwrap()
        .available = ReadinessState::Unavailable;
    assert!(!readiness.supports_role(HarnessRole::Implementer));
}

#[test]
fn results_validate_failure_status_and_bounded_artifact_manifest() {
    let mut result = ExecutionResultV1 {
        schema_version: 1,
        execution_id: id(),
        exit_code: None,
        signal: None,
        timed_out: true,
        cancelled: false,
        metrics: ResourceMetricsV1 {
            cpu_millis: 100,
            peak_memory_bytes: 1024,
            process_count: 1,
            disk_bytes: 0,
        },
        phase_timings: Default::default(),
        artifacts: vec![ArtifactManifestEntryV1 {
            relative_path: "evidence/result.json".into(),
            size_bytes: 32,
            sha256: "a".repeat(64),
            media_type: "application/json".into(),
        }],
        failure: Some(ExecutionError::new(
            ExecutionErrorKind::DeadlineExceeded,
            "execution deadline elapsed",
            false,
        )),
    };
    result.validate().unwrap();
    result.artifacts[0].relative_path = "../outside".into();
    assert!(result.validate().is_err());
    result.artifacts.clear();
    result.failure.as_mut().unwrap().message = "x".repeat(4097);
    assert!(result.validate().is_err());
}
