use super::{availability_label, can_select_default, implementation_guidance};
use crate::{
    execution::contract::{
        HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ReadinessState, RoleReadinessV1,
    },
    persistence::harness_settings::DetectedHarness,
};

fn opencode(model_catalog: ReadinessState, fresh: bool) -> DetectedHarness {
    DetectedHarness {
        status: "opencode ready".into(),
        version: Some("1.0.0".into()),
        executable: Some("/example/bin/opencode".into()),
        diagnostic: None,
        ready: true,
        models: vec!["gpt".into()],
        default_model: Some("gpt".into()),
        model_catalog_fresh: fresh,
        configuration_required: false,
        implementation_available: true,
        checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
        capability_snapshot: Some(HarnessCapabilitySnapshotV1 {
            schema_version: 1,
            provider_id: "opencode".into(),
            probe_id: "123e4567-e89b-12d3-a456-426614174000".into(),
            probe_cancellation_generation: 1,
            probe_sequence: 1,
            cli_binary: ReadinessState::Ready,
            cli_version: Some("1.0.0".into()),
            authentication: ReadinessState::Ready,
            model_catalog,
            scriptable_invocation: ReadinessState::Ready,
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
            roles: HarnessRole::ALL
                .into_iter()
                .map(|role| RoleReadinessV1 {
                    role,
                    available: ReadinessState::Unknown,
                    failure: None,
                })
                .collect(),
        }),
    }
}

#[test]
fn authenticated_cli_with_stale_models_is_not_shown_as_available_or_selectable() {
    let harness = opencode(ReadinessState::Unknown, false);
    let now = crate::persistence::harness_settings::current_unix_ms();

    assert!(harness.has_current_authenticated_capability("opencode", now));
    assert_eq!(
        availability_label("opencode", &harness, now, false),
        "Authenticated; model access unverified"
    );
    assert!(!can_select_default("opencode", &harness, now));
}

#[test]
fn fresh_models_without_a_verified_context_only_role_are_not_available() {
    let harness = opencode(ReadinessState::Ready, true);
    let now = crate::persistence::harness_settings::current_unix_ms();

    assert_eq!(
        availability_label("opencode", &harness, now, false),
        "Context-only mode unavailable"
    );
    assert!(!can_select_default("opencode", &harness, now));
}

#[test]
fn listed_models_without_a_verified_cli_default_cannot_become_the_global_default() {
    let mut harness = opencode(ReadinessState::Ready, true);
    harness.default_model = None;
    let now = crate::persistence::harness_settings::current_unix_ms();

    assert_eq!(
        availability_label("opencode", &harness, now, false),
        "Choose a model; CLI default is unverified"
    );
    assert!(!can_select_default("opencode", &harness, now));
}

#[test]
fn context_only_planning_role_can_be_default_without_repository_execution() {
    let mut harness = opencode(ReadinessState::Ready, true);
    harness.implementation_available = false;
    let snapshot = harness.capability_snapshot.as_mut().unwrap();
    snapshot.docker_mcp_bridge = ReadinessState::Unavailable;
    snapshot.execution_target = ReadinessState::Unavailable;
    for mode in &mut snapshot.modes {
        if mode.mode == crate::harness::ExecutionMode::Planning {
            mode.available = ReadinessState::Ready;
        }
    }
    for role in &mut snapshot.roles {
        if role.role == HarnessRole::PlanningAnalyst {
            role.available = ReadinessState::Unavailable;
        }
    }
    assert!(!snapshot.supports_role(HarnessRole::PlanningAnalyst));
    assert!(snapshot.supports_context_only_planning());
    let now = crate::persistence::harness_settings::current_unix_ms();

    assert_eq!(
        availability_label("opencode", &harness, now, false),
        "Available"
    );
    assert!(can_select_default("opencode", &harness, now));
    assert!(!harness.implementation_available);
    assert!(implementation_guidance("opencode").contains("Repository execution is unavailable"));
    assert!(implementation_guidance("opencode").contains("Context-only planning"));
    assert!(implementation_guidance("opencode").contains("ready Docker execution session"));
}

#[test]
fn authenticated_claude_stays_unavailable_and_cannot_become_default() {
    let mut harness = opencode(ReadinessState::Ready, true);
    let snapshot = harness.capability_snapshot.as_mut().unwrap();
    snapshot.provider_id = "claude".into();
    let now = crate::persistence::harness_settings::current_unix_ms();

    assert_eq!(
        availability_label("claude", &harness, now, false),
        "Provider execution unavailable"
    );
    assert!(!can_select_default("claude", &harness, now));
}

#[test]
fn implementation_guidance_names_the_provider_specific_blocker() {
    assert!(implementation_guidance("claude").contains("organization-managed hooks"));
    assert!(implementation_guidance("codex").contains("organization-managed hooks"));
}
