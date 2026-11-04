use super::*;
use crate::execution::contract::{
    HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ReadinessState, RoleReadinessV1,
};
fn fixture() -> DlgHarnessSetup {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let harness = crate::persistence::harness_settings::DetectedHarness {
        status: "pi ready".into(),
        version: Some("1.0.0".into()),
        executable: Some("/synthetic/bin/pi".into()),
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
            provider_id: "pi".into(),
            authentication_mode: crate::execution::contract::current_authentication_mode("pi"),
            manager_profile_id:
                crate::execution::contract::profile_evidence_id_for_current_snapshot(
                    "pi",
                    Some("1.0.0"),
                    crate::harness::ExecutionProfile::Manager,
                ),
            worker_profile_id: crate::execution::contract::profile_evidence_id_for_current_snapshot(
                "pi",
                Some("1.0.0"),
                crate::harness::ExecutionProfile::Worker,
            ),
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
                    available: if matches!(
                        mode,
                        crate::harness::ExecutionMode::Manager
                            | crate::harness::ExecutionMode::Planning
                    ) {
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
    let mut settings = harness_settings::HarnessSettings::default();
    settings.discovered.insert("pi".into(), harness);
    settings.default_harness = Some("codex".into());
    settings.work_routes.insert(
        "implementation".into(),
        WorkRoute {
            harness: "codex".into(),
            model: Some("custom-model".into()),
        },
    );
    DlgHarnessSetup::review_fixture(settings)
}
#[test]
fn helper_choice_before_docker_does_not_grant_manager_readiness() {
    let mut dialog = fixture();
    let before = dialog.settings.clone();
    dialog.select_onboarding_manager("pi");
    assert_eq!(dialog.settings.work_routes[MANAGER].harness, "pi");
    assert_eq!(dialog.settings.bootstrap_provider.as_deref(), Some("pi"));
    assert!(!dialog.onboarding_manager_ready("pi"));
    assert_eq!(dialog.settings.discovered, before.discovered);
    assert_eq!(
        dialog.settings.work_routes["implementation"],
        before.work_routes["implementation"]
    );
}
#[test]
fn stale_or_unauthenticated_provider_cannot_change_routes() {
    let mut dialog = fixture();
    dialog
        .settings
        .discovered
        .get_mut("pi")
        .unwrap()
        .checked_at_unix_ms = 0;
    let before = dialog.settings.clone();
    dialog.select_onboarding_manager("pi");
    assert_eq!(dialog.settings, before);
    let mut dialog = fixture();
    dialog
        .settings
        .discovered
        .get_mut("pi")
        .unwrap()
        .capability_snapshot
        .as_mut()
        .unwrap()
        .authentication = ReadinessState::Unknown;
    let before = dialog.settings.clone();
    dialog.select_onboarding_manager("pi");
    assert_eq!(dialog.settings, before);
}

#[test]
fn worker_only_provider_cannot_be_selected_for_manager() {
    let mut dialog = fixture();
    let mut harness = dialog.settings.discovered.get("pi").unwrap().clone();
    harness.capability_snapshot.as_mut().unwrap().provider_id = "antigravity".into();
    harness
        .capability_snapshot
        .as_mut()
        .unwrap()
        .authentication_mode =
        crate::execution::contract::current_authentication_mode("antigravity");
    let snapshot = harness.capability_snapshot.as_mut().unwrap();
    snapshot.manager_profile_id = crate::execution::contract::profile_evidence_id_for_snapshot(
        "antigravity",
        snapshot.cli_version.as_deref(),
        snapshot.authentication_mode,
        crate::harness::ExecutionProfile::Manager,
    );
    snapshot.worker_profile_id = crate::execution::contract::profile_evidence_id_for_snapshot(
        "antigravity",
        snapshot.cli_version.as_deref(),
        snapshot.authentication_mode,
        crate::harness::ExecutionProfile::Worker,
    );
    dialog.settings.discovered.remove("pi");
    dialog
        .settings
        .discovered
        .insert("antigravity".into(), harness.clone());
    let before = dialog.settings.clone();

    assert!(dialog.onboarding_model("antigravity").is_some());
    assert!(!dialog.onboarding_manager_profile_available("antigravity"));
    dialog.select_onboarding_manager("antigravity");
    assert_eq!(dialog.settings.work_routes, before.work_routes);
    assert_eq!(
        dialog.settings.bootstrap_provider,
        before.bootstrap_provider
    );
    assert!(
        dialog
            .feedback
            .as_ref()
            .unwrap()
            .1
            .contains("not for Man.ager")
    );
}

#[test]
fn verified_manager_and_tool_free_help_can_coexist_without_changing_other_routes() {
    let mut dialog = fixture();
    let snapshot = dialog
        .settings
        .discovered
        .get_mut("pi")
        .unwrap()
        .capability_snapshot
        .as_mut()
        .unwrap();
    snapshot.execution_target = ReadinessState::Ready;
    snapshot.docker_mcp_bridge = ReadinessState::Ready;
    snapshot
        .roles
        .iter_mut()
        .find(|role| role.role == HarnessRole::Manager)
        .unwrap()
        .available = ReadinessState::Ready;
    assert!(snapshot.supports_role(HarnessRole::Manager));
    assert!(snapshot.supports_context_only_help());
    assert!(!snapshot.supports_context_only_planning());
    let before = dialog.settings.clone();
    dialog.select_onboarding_manager("pi");
    assert_eq!(dialog.settings.work_routes[MANAGER].harness, "pi");
    assert_eq!(dialog.settings.bootstrap_provider.as_deref(), Some("pi"));
    assert_eq!(dialog.settings.default_harness, before.default_harness);
    assert_eq!(
        dialog.settings.work_routes["implementation"],
        before.work_routes["implementation"]
    );
    assert_eq!(dialog.settings.discovered, before.discovered);
}
