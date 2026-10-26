use super::*;
use crate::execution::contract::{
    HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ReadinessState, RoleReadinessV1,
};

#[test]
fn context_only_planning_is_available_in_role_route_picker_without_docker() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let mut harness = crate::persistence::harness_settings::DetectedHarness {
        status: "opencode ready".into(),
        version: Some("1.0.0".into()),
        executable: Some("/synthetic/bin/opencode".into()),
        diagnostic: None,
        ready: true,
        models: vec!["local-test-model".into()],
        default_model: Some("local-test-model".into()),
        model_catalog_fresh: true,
        configuration_required: false,
        implementation_available: false,
        checked_at_unix_ms: now,
        capability_snapshot: Some(HarnessCapabilitySnapshotV1 {
            schema_version: 1,
            provider_id: "opencode".into(),
            probe_id: "123e4567-e89b-12d3-a456-426614174000".into(),
            probe_cancellation_generation: 1,
            probe_sequence: 1,
            cli_binary: ReadinessState::Ready,
            cli_version: Some("1.0.0".into()),
            authentication: ReadinessState::Ready,
            model_catalog: ReadinessState::Ready,
            scriptable_invocation: ReadinessState::Ready,
            docker_mcp_bridge: ReadinessState::Unavailable,
            execution_target: ReadinessState::Unavailable,
            failure: None,
            modes: crate::harness::ExecutionMode::ALL
                .into_iter()
                .map(|mode| ModeReadinessV1 {
                    mode,
                    available: if mode == crate::harness::ExecutionMode::Planning {
                        ReadinessState::Ready
                    } else {
                        ReadinessState::Unknown
                    },
                    failure: None,
                })
                .collect(),
            roles: HarnessRole::ALL
                .into_iter()
                .map(|role| RoleReadinessV1 {
                    role,
                    available: ReadinessState::Unavailable,
                    failure: None,
                })
                .collect(),
        }),
    };

    assert!(route_available(
        "opencode",
        &harness,
        crate::persistence::harness_settings::PLANNING
    ));
    assert!(!route_available(
        "opencode",
        &harness,
        crate::persistence::harness_settings::IMPLEMENTATION
    ));

    harness.default_model = None;
    assert!(route_available(
        "opencode",
        &harness,
        crate::persistence::harness_settings::PLANNING
    ));
    harness.models.clear();
    assert!(!route_available(
        "opencode",
        &harness,
        crate::persistence::harness_settings::PLANNING
    ));
}
