use super::{super::*, temp_file};
use crate::execution::contract::{
    HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ReadinessState, RoleReadinessV1,
};
use std::collections::BTreeMap;

fn detected(provider: &str) -> DetectedHarness {
    DetectedHarness {
        status: format!("{provider} ready"),
        version: Some("1.0.0".into()),
        executable: Some(format!("/tools/{provider}")),
        diagnostic: None,
        ready: true,
        models: vec!["model-a".into()],
        default_model: Some("model-a".into()),
        model_catalog_fresh: true,
        configuration_required: false,
        implementation_available: true,
        checked_at_unix_ms: current_unix_ms(),
        capability_snapshot: Some(HarnessCapabilitySnapshotV1 {
            schema_version: 1,
            provider_id: provider.into(),
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
        }),
    }
}

#[test]
fn schema_four_revokes_provider_evidence_whose_meaning_changed() {
    let path = temp_file();
    let settings = HarnessSettings {
        schema_version: 4,
        default_harness: Some("codex".into()),
        discovered: ["pi", "codex", "claude", "opencode", "copilot"]
            .into_iter()
            .map(|provider| (provider.into(), detected(provider)))
            .collect::<BTreeMap<_, _>>(),
        ..HarnessSettings::default()
    };
    assert!(
        settings.discovered["copilot"]
            .capability_snapshot
            .as_ref()
            .unwrap()
            .supports_role(HarnessRole::DecisionExplainer)
    );
    std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();

    let (migrated, diagnostic) = load_from(&path);

    assert!(diagnostic.is_none());
    assert_eq!(migrated.schema_version, 5);
    assert_eq!(migrated.default_harness.as_deref(), Some("codex"));
    for provider in ["codex", "claude", "opencode"] {
        let detected = &migrated.discovered[provider];
        assert_eq!(
            detected
                .capability_snapshot
                .as_ref()
                .unwrap()
                .authentication,
            ReadinessState::Unknown,
            "schema 4 {provider} auth was not tied to current account access"
        );
        assert!(!detected.has_current_authenticated_capability(provider, current_unix_ms()));
    }
    for provider in ["codex", "claude", "copilot"] {
        let detected = &migrated.discovered[provider];
        assert!(!detected.model_catalog_fresh);
        assert_eq!(
            detected.model_catalog_status(),
            crate::harness::discovery::ModelCatalogStatus::Stale
        );
        assert_eq!(
            detected.capability_snapshot.as_ref().unwrap().model_catalog,
            ReadinessState::Unknown
        );
    }
    assert!(
        !migrated.discovered["copilot"]
            .capability_snapshot
            .as_ref()
            .unwrap()
            .supports_role(HarnessRole::DecisionExplainer)
    );
    assert_eq!(
        migrated.discovered["opencode"].model_catalog_status(),
        crate::harness::discovery::ModelCatalogStatus::Fresh
    );
    assert_eq!(
        migrated.discovered["pi"]
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        ReadinessState::Ready,
        "unaffected provider evidence is preserved"
    );
    std::fs::remove_file(path).unwrap();
}
