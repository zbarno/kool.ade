use crate::execution::contract::{
    ArtifactManifestEntryV1, ExecutionResultV1, HarnessRole, ProviderReadinessV1, ReadinessState,
    ResourceMetricsV1, RoleReadinessV1,
};
use crate::execution::errors::{ExecutionError, ExecutionErrorKind};

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
        roles: vec![RoleReadinessV1 {
            role: HarnessRole::Documentation,
            available: ReadinessState::Ready,
            failure: None,
        }],
    };
    readiness.validate().unwrap();
    readiness.roles.push(readiness.roles[0].clone());
    assert!(readiness.validate().is_err());
    assert_eq!(
        HarnessRole::Documentation.harness_mode(),
        crate::harness::ExecutionMode::ReadOnlyAnalysis
    );
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
