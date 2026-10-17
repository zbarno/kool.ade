use super::*;

pub(crate) fn capability_snapshot(
    provider_id: &str,
    authentication: crate::execution::contract::ReadinessState,
) -> crate::execution::contract::HarnessCapabilitySnapshotV1 {
    use crate::execution::contract::{
        HarnessCapabilitySnapshotV1, ModeReadinessV1, ReadinessState, RoleReadinessV1,
    };

    HarnessCapabilitySnapshotV1 {
        schema_version: 1,
        provider_id: provider_id.into(),
        probe_id: "00000000-0000-0000-0000-000000000001".into(),
        probe_cancellation_generation: 1,
        probe_sequence: 1,
        cli_binary: ReadinessState::Ready,
        cli_version: Some("1.0.0".into()),
        authentication,
        model_catalog: ReadinessState::Ready,
        scriptable_invocation: ReadinessState::Unknown,
        docker_mcp_bridge: ReadinessState::Unavailable,
        execution_target: ReadinessState::Unavailable,
        failure: None,
        modes: crate::harness::ExecutionMode::ALL
            .into_iter()
            .map(|mode| ModeReadinessV1 {
                mode,
                available: ReadinessState::Unknown,
                failure: None,
            })
            .collect(),
        roles: crate::execution::contract::HarnessRole::ALL
            .into_iter()
            .map(|role| RoleReadinessV1 {
                role,
                available: ReadinessState::Unknown,
                failure: None,
            })
            .collect(),
    }
}

#[test]
fn unknown_provider_authentication_blocks_route_validation_and_task_execution() {
    use crate::execution::contract::ReadinessState;
    use crate::persistence::harness_settings::{DetectedHarness, HarnessSettings, WorkRoute};

    for provider_id in ["pi", "copilot", "opencode", "antigravity"] {
        let detected = DetectedHarness {
            status: format!("{provider_id} ready"),
            version: Some("1.0.0".into()),
            executable: Some(format!("/tools/{provider_id}")),
            diagnostic: None,
            ready: true,
            models: vec![],
            default_model: None,
            model_catalog_fresh: false,
            configuration_required: false,
            implementation_available: true,
            checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
            capability_snapshot: Some(capability_snapshot(provider_id, ReadinessState::Unknown)),
        };
        let settings = HarnessSettings {
            discovered: std::collections::BTreeMap::from([(provider_id.into(), detected)]),
            ..Default::default()
        };
        let route = WorkRoute {
            harness: provider_id.into(),
            model: None,
        };
        let routes = std::collections::BTreeMap::from([(
            crate::persistence::harness_settings::IMPLEMENTATION.into(),
            route.clone(),
        )]);

        let validation_error = validate_task_routes(&settings, &routes).unwrap_err();
        assert!(
            validation_error.contains("Authentication"),
            "{validation_error}"
        );
        let task_harness = routed_harness(
            &settings,
            Some(crate::persistence::harness_settings::IMPLEMENTATION),
            None,
            Some(&route),
        );
        let execution_error = task_harness.check_available().unwrap_err().detail();
        assert!(
            execution_error.contains("Authentication"),
            "{execution_error}"
        );
        assert!(execution_error.contains("no fallback"), "{execution_error}");
    }
}
