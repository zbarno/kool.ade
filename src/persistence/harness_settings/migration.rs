use super::{CURRENT_SCHEMA_VERSION, HarnessSettings};
use crate::execution::contract::ReadinessState;

pub(super) fn apply(settings: &mut HarnessSettings, saved_schema_version: u8) {
    if saved_schema_version < 4 {
        for detected in settings.discovered.values_mut() {
            if let Some(snapshot) = detected.capability_snapshot.as_mut() {
                snapshot.authentication = ReadinessState::Unknown;
            }
        }
    }

    if saved_schema_version >= CURRENT_SCHEMA_VERSION {
        return;
    }

    for (provider, detected) in &mut settings.discovered {
        let invalidate_authentication =
            matches!(provider.as_str(), "codex" | "claude" | "opencode");
        let invalidate_model_catalog = matches!(provider.as_str(), "codex" | "claude" | "copilot");
        if invalidate_authentication && let Some(snapshot) = detected.capability_snapshot.as_mut() {
            snapshot.authentication = ReadinessState::Unknown;
        }
        if invalidate_model_catalog {
            detected.model_catalog_fresh = false;
            if let Some(snapshot) = detected.capability_snapshot.as_mut() {
                snapshot.model_catalog = ReadinessState::Unknown;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::contract::{HarnessCapabilitySnapshotV1, ReadinessState};

    #[test]
    fn schema_four_revokes_auth_and_model_evidence_after_probe_semantics_change() {
        let mut settings = HarnessSettings {
            discovered: std::collections::BTreeMap::from([(
                "codex".into(),
                crate::persistence::harness_settings::DetectedHarness {
                    status: "codex ready".into(),
                    version: Some("1.0.0".into()),
                    executable: Some("/synthetic/bin/codex".into()),
                    diagnostic: None,
                    ready: true,
                    models: vec!["synthetic-model".into()],
                    default_model: Some("synthetic-model".into()),
                    model_catalog_fresh: true,
                    configuration_required: false,
                    implementation_available: false,
                    checked_at_unix_ms: 1,
                    capability_snapshot: Some(HarnessCapabilitySnapshotV1 {
                        schema_version: 1,
                        provider_id: "codex".into(),
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
                            .map(|mode| crate::execution::contract::ModeReadinessV1 {
                                mode,
                                available: ReadinessState::Ready,
                                failure: None,
                            })
                            .collect(),
                        roles: crate::execution::contract::HarnessRole::ALL
                            .into_iter()
                            .map(|role| crate::execution::contract::RoleReadinessV1 {
                                role,
                                available: ReadinessState::Ready,
                                failure: None,
                            })
                            .collect(),
                    }),
                },
            )]),
            ..HarnessSettings::default()
        };

        apply(&mut settings, 4);

        let detected = &settings.discovered["codex"];
        let snapshot = detected.capability_snapshot.as_ref().unwrap();
        assert_eq!(snapshot.authentication, ReadinessState::Unknown);
        assert_eq!(snapshot.model_catalog, ReadinessState::Unknown);
        assert!(!detected.model_catalog_fresh);
    }
}
