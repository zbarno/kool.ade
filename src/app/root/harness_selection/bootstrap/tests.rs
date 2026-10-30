use super::*;
use crate::execution::contract::{
    HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ReadinessState, RoleReadinessV1,
};
fn request() -> crate::harness::PlanningRequest {
    crate::harness::PlanningRequest {
        prompt_manifest: Some(crate::harness::prompt_catalog::PromptManifest {
            role_version: "bootstrap_conversation/v1".into(),
            system_template_sha256: String::new(),
            policy_template_sha256: String::new(),
            user_template_sha256: String::new(),
            output_schema_version: String::new(),
            privilege_profile: String::new(),
        }),
        mode: crate::harness::ExecutionMode::ReadOnlyAnalysis,
        task_id: None,
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        reasoning_level: "high".into(),
        telemetry_phase: None,
        repo_root: std::path::PathBuf::from("/synthetic/project"),
        runtime_config_source: None,
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx: std::sync::mpsc::channel().0,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        review_identity: None,
    }
}

#[test]
fn selected_helper_ignores_unrelated_default_and_rejects_repository_requests() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let harness = crate::persistence::harness_settings::DetectedHarness {
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

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    settings.discovered.insert("opencode".into(), harness);
    settings.default_harness = Some("codex".into());
    settings.bootstrap_provider = Some("opencode".into());
    crate::persistence::harness_settings::save(&settings).unwrap();
    let route = configured_bootstrap_harness(&mut None);
    let mut request = request();
    assert_eq!(
        route.route_provenance(&request).unwrap().provider_id,
        "opencode"
    );
    request.mode = crate::harness::ExecutionMode::Planning;
    assert!(route.route_provenance(&request).is_none());
    assert!(
        route
            .execute(&request)
            .unwrap_err()
            .to_string()
            .contains("Setup help cannot execute")
    );
    request.mode = crate::harness::ExecutionMode::ReadOnlyAnalysis;
    request.runtime_config_source = Some("/synthetic/project".into());
    assert!(route.execute(&request).is_err());
    settings.bootstrap_provider = None;
    crate::persistence::harness_settings::save(&settings).unwrap();
    assert!(
        configured_bootstrap_harness(&mut None)
            .route_provenance(&request)
            .is_none()
    );
}
