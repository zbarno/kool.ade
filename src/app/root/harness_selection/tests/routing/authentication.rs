use super::*;
use crate::execution::contract::{
    HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ReadinessState, RoleReadinessV1,
};
use crate::persistence::harness_settings::{
    DetectedHarness, HarnessSettings, IMPLEMENTATION, WorkRoute,
};

#[path = "authentication/implementation_routes.rs"]
mod implementation_routes;
#[cfg(unix)]
#[path = "authentication/opencode_route.rs"]
mod opencode_route;

pub(crate) fn ready_snapshot(provider: &str) -> HarnessCapabilitySnapshotV1 {
    snapshot(provider, ReadinessState::Ready)
}

pub(crate) fn capability_snapshot(
    provider: &str,
    authentication: ReadinessState,
) -> HarnessCapabilitySnapshotV1 {
    snapshot(provider, authentication)
}

fn snapshot(provider: &str, auth: ReadinessState) -> HarnessCapabilitySnapshotV1 {
    HarnessCapabilitySnapshotV1 {
        schema_version: 1,
        provider_id: provider.into(),
        probe_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        probe_cancellation_generation: 1,
        probe_sequence: 1,
        cli_binary: ReadinessState::Ready,
        cli_version: Some("1.0.0".into()),
        authentication: auth,
        model_catalog: ReadinessState::Ready,
        scriptable_invocation: ReadinessState::Ready,
        docker_mcp_bridge: ReadinessState::Ready,
        execution_target: ReadinessState::Ready,
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
    }
}

fn detected(provider: &str, auth: ReadinessState, checked_at_unix_ms: u64) -> DetectedHarness {
    DetectedHarness {
        status: format!("{provider} ready"),
        version: Some("1.0.0".into()),
        executable: Some(format!("/synthetic/bin/{provider}")),
        diagnostic: None,
        ready: true,
        models: vec![],
        default_model: None,
        model_catalog_fresh: true,
        configuration_required: false,
        implementation_available: true,
        checked_at_unix_ms,
        capability_snapshot: Some(snapshot(provider, auth)),
    }
}

fn settings_with(provider: &str, auth: ReadinessState, checked_at_unix_ms: u64) -> HarnessSettings {
    HarnessSettings {
        schema_version: 3,
        default_harness: Some(provider.into()),
        discovered: std::collections::BTreeMap::from([(
            provider.into(),
            detected(provider, auth, checked_at_unix_ms),
        )]),
        ..Default::default()
    }
}

fn assert_configured_route_rejected(settings: &HarnessSettings) {
    let path =
        std::env::temp_dir().join(format!("harness-route-auth-{}.json", uuid::Uuid::new_v4()));
    crate::persistence::harness_settings::with_settings_path(path.clone(), || {
        crate::persistence::harness_settings::save(settings).unwrap();
        let mut override_harness = None;
        let harness = configured_harness_for_task(
            &mut override_harness,
            Some(IMPLEMENTATION),
            &std::collections::BTreeMap::new(),
        );
        let error = harness.check_available().unwrap_err().detail();
        assert!(
            error.contains("authenticated capability snapshot"),
            "{error}"
        );
    });
    let _ = std::fs::remove_file(path);
}

#[test]
fn configured_implementation_route_requires_current_ready_authentication_snapshot() {
    use crate::persistence::harness_settings::DISCOVERY_FRESHNESS_MS;
    let now = crate::persistence::harness_settings::current_unix_ms();

    let mut missing = settings_with("codex", ReadinessState::Ready, now);
    missing
        .discovered
        .get_mut("codex")
        .unwrap()
        .capability_snapshot = None;
    assert_configured_route_rejected(&missing);

    let unknown = settings_with("codex", ReadinessState::Unknown, now);
    assert_configured_route_rejected(&unknown);

    let stale = settings_with(
        "codex",
        ReadinessState::Ready,
        now.saturating_sub(DISCOVERY_FRESHNESS_MS + 1),
    );
    let path = std::env::temp_dir().join(format!(
        "harness-route-auth-stale-{}.json",
        uuid::Uuid::new_v4()
    ));
    crate::persistence::harness_settings::with_settings_path(path.clone(), || {
        crate::persistence::harness_settings::save(&stale).unwrap();
        let mut override_harness = None;
        let harness = configured_harness_for_task(
            &mut override_harness,
            Some(IMPLEMENTATION),
            &std::collections::BTreeMap::new(),
        );
        let error = harness.check_available().unwrap_err().detail();
        assert!(error.contains("stale"), "{error}");
    });
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_route_validation_requires_typed_ready_authentication() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let route = std::collections::BTreeMap::from([(
        IMPLEMENTATION.into(),
        WorkRoute {
            harness: "codex".into(),
            model: None,
        },
    )]);

    for auth in [ReadinessState::Unknown, ReadinessState::Unavailable] {
        let settings = settings_with("codex", auth, now);
        let error = validate_task_routes(&settings, &route).unwrap_err();
        assert!(
            error.contains("authenticated capability snapshot"),
            "{error}"
        );
    }

    let stale = settings_with(
        "codex",
        ReadinessState::Ready,
        now.saturating_sub(crate::persistence::harness_settings::DISCOVERY_FRESHNESS_MS + 1),
    );
    assert!(
        validate_task_routes(&stale, &route)
            .unwrap_err()
            .contains("discovery is stale")
    );

    let mut missing = settings_with("codex", ReadinessState::Ready, now);
    missing
        .discovered
        .get_mut("codex")
        .unwrap()
        .capability_snapshot = None;
    assert!(
        validate_task_routes(&missing, &route)
            .unwrap_err()
            .contains("authenticated capability snapshot")
    );
}

#[test]
fn uncategorized_turn_does_not_fall_back_around_an_explicit_implementation_route() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let mut settings = settings_with("pi", ReadinessState::Ready, now);
    settings.discovered.insert(
        "codex".into(),
        detected("codex", ReadinessState::Unknown, now),
    );
    let routes = std::collections::BTreeMap::from([(
        IMPLEMENTATION.into(),
        WorkRoute {
            harness: "codex".into(),
            model: None,
        },
    )]);
    settings
        .work_routes
        .insert(IMPLEMENTATION.into(), routes[IMPLEMENTATION].clone());
    let empty_routes = std::collections::BTreeMap::new();
    for task_routes in [&empty_routes, &routes] {
        let path = std::env::temp_dir().join(format!(
            "harness-route-auth-uncategorized-{}.json",
            uuid::Uuid::new_v4()
        ));
        crate::persistence::harness_settings::with_settings_path(path.clone(), || {
            crate::persistence::harness_settings::save(&settings).unwrap();
            let mut override_harness = None;
            let harness = configured_harness_for_task(&mut override_harness, None, task_routes);
            assert!(harness.label().to_ascii_lowercase().contains("codex"));
            let error = harness.check_available().unwrap_err().detail();
            assert!(
                error.contains("authenticated capability snapshot"),
                "{error}"
            );
        });
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn unknown_provider_authentication_blocks_route_validation_and_task_execution() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    for provider in ["pi", "copilot", "opencode", "antigravity"] {
        let settings = settings_with(provider, ReadinessState::Unknown, now);
        let route = WorkRoute {
            harness: provider.into(),
            model: None,
        };
        let routes = std::collections::BTreeMap::from([(IMPLEMENTATION.into(), route.clone())]);

        let validation_error = validate_task_routes(&settings, &routes).unwrap_err();
        assert!(validation_error.contains("Authentication"), "{validation_error}");
        let task_harness = routed_harness(
            &settings,
            Some(IMPLEMENTATION),
            None,
            Some(&route),
        );
        let execution_error = task_harness.check_available().unwrap_err().detail();
        assert!(execution_error.contains("Authentication"), "{execution_error}");
        assert!(execution_error.contains("no fallback"), "{execution_error}");
    }
}
