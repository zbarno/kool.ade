use super::*;
use crate::execution::contract::{
    HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ReadinessState, RoleReadinessV1,
};

#[test]
fn project_routes_require_a_verified_default_or_offer_only_listed_models() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let mut entry = crate::persistence::harness_settings::DetectedHarness {
        status: "opencode ready".into(),
        version: Some("1.0.0".into()),
        executable: Some("/synthetic/bin/opencode".into()),
        diagnostic: None,
        ready: true,
        models: vec!["listed-model".into()],
        default_model: Some("unlisted-model".into()),
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

    let candidates = model_routes_for("opencode", &entry, now);
    assert_eq!(
        candidates,
        vec![WorkRoute {
            harness: "opencode".into(),
            model: Some("listed-model".into()),
        }]
    );

    let mut settings = crate::persistence::harness_settings::HarnessSettings {
        discovered: BTreeMap::from([("opencode".into(), entry.clone())]),
        ..Default::default()
    };
    assert_eq!(
        route_label(
            &WorkRoute {
                harness: "opencode".into(),
                model: Some("listed-model".into()),
            },
            &settings,
        ),
        "opencode / listed-model"
    );
    assert_eq!(
        route_label(
            &WorkRoute {
                harness: "opencode".into(),
                model: Some("unlisted-model".into()),
            },
            &settings,
        ),
        "opencode / unlisted-model (unverified)"
    );

    settings
        .discovered
        .get_mut("opencode")
        .expect("fixture remains discovered")
        .model_catalog_fresh = false;
    assert_eq!(
        route_label(
            &WorkRoute {
                harness: "opencode".into(),
                model: Some("listed-model".into()),
            },
            &settings,
        ),
        "opencode / listed-model (unverified)"
    );

    entry.models.clear();
    assert!(model_routes_for("opencode", &entry, now).is_empty());
}
